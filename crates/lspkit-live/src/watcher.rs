//! Filesystem watcher that emits coarse change events.
//!
//! Wraps [`notify`] and forwards events to a tokio channel so consumers can
//! await them with the usual async machinery. Exclusion rules let callers
//! ignore noisy directories (`target/`, `.git/`, vendor caches, etc.).

use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

/// A coarse change event suitable for triggering re-analysis.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ChangeEvent {
    /// One or more paths under the watched root were created or modified.
    Touched(Vec<PathBuf>),
    /// One or more paths were removed.
    Removed(Vec<PathBuf>),
    /// Native observation was incomplete; consumers must rescan their owned inputs.
    /// Implements [LSP-PULL-WATCH]. No path subset can represent the lost changes.
    Rescan,
}

/// Errors from setting up or running the watcher.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum WatcherError {
    /// The filesystem watcher could not be created.
    #[error("watcher setup failed: {0}")]
    Setup(String),
    /// A path could not be watched.
    #[error("watch failed: {0}")]
    Watch(String),
}

/// A filesystem watcher that streams [`ChangeEvent`] values.
pub struct FileWatcher {
    /// Keeps native subscriptions active for as long as the stream is owned.
    _inner: RecommendedWatcher,
    /// Receives coarse events forwarded from the native watcher thread.
    rx: mpsc::UnboundedReceiver<ChangeEvent>,
}

/// Owned watcher and shared debounce stream; dropping it releases native watching.
#[derive(Debug)]
pub struct CoalescedWatcher {
    /// Yields batches after the shared scheduler's quiet or capped window.
    triggers: mpsc::UnboundedReceiver<crate::scheduler::Trigger>,
    /// Owns the async pump so dropping this value stops reading native events.
    reader: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for FileWatcher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FileWatcher")
            .finish_non_exhaustive()
    }
}

impl CoalescedWatcher {
    /// Wait for the next coalesced input batch.
    pub async fn next(&mut self) -> Option<crate::scheduler::Trigger> {
        self.triggers.recv().await
    }
}

impl Drop for CoalescedWatcher {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl FileWatcher {
    /// Feed the canonical shared debouncer while retaining native watcher ownership.
    #[must_use]
    pub fn coalesced(mut self, config: crate::scheduler::ScheduleConfig) -> CoalescedWatcher {
        let (sender, receiver) = mpsc::unbounded_channel();
        let reader = tokio::spawn(async move {
            while let Some(event) = self.next().await {
                if sender.send(event).is_err() {
                    return;
                }
            }
        });
        CoalescedWatcher {
            triggers: crate::scheduler::spawn(receiver, config),
            reader,
        }
    }
    /// Start watching `root` recursively. Returns a watcher that streams events.
    ///
    /// # Errors
    /// Returns [`WatcherError`] if the watcher cannot be created or the path
    /// cannot be watched.
    pub fn new(root: &Path) -> Result<Self, WatcherError> {
        Self::with_mode(root, RecursiveMode::Recursive)
    }

    /// Watch only immediate children, without subscribing to descendant trees.
    /// Implements [LSP-PULL-WATCH].
    ///
    /// # Errors
    /// Returns [`WatcherError`] if the watcher cannot be created or attached.
    pub fn non_recursive(root: &Path) -> Result<Self, WatcherError> {
        Self::with_mode(root, RecursiveMode::NonRecursive)
    }

    /// Both depth modes share native setup, event mapping and owned channel lifetime.
    fn with_mode(root: &Path, mode: RecursiveMode) -> Result<Self, WatcherError> {
        let (event_tx, event_rx) = std_mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(event_tx)
            .map_err(|e| WatcherError::Setup(e.to_string()))?;
        watcher
            .watch(root, mode)
            .map_err(|e| WatcherError::Watch(e.to_string()))?;
        let alias = RootAlias::of(root)?;
        let (out_tx, rx) = mpsc::unbounded_channel();
        let _forwarder = std::thread::Builder::new()
            .name("lspkit-watch".to_owned())
            .spawn(move || forward_events(event_rx, &out_tx, &alias))
            .map_err(|error| WatcherError::Setup(error.to_string()))?;
        Ok(Self {
            _inner: watcher,
            rx,
        })
    }

    /// Await the next change event. Returns `None` when the watcher is dropped.
    pub async fn next(&mut self) -> Option<ChangeEvent> {
        self.rx.recv().await
    }
}

/// The watched root as the caller gave it and as the OS reports it. macOS reports a
/// root given as `/var/…` under `/private/var/…`; every event is rebased onto the
/// given root so consumers only ever see paths under the root they asked for.
/// Implements [LSP-PULL-WATCH].
struct RootAlias {
    /// The root exactly as the caller passed it.
    given: PathBuf,
    /// The root with every symbolic link resolved, as native events report it.
    reported: PathBuf,
}

impl RootAlias {
    /// Resolve `root` once, when the native watch has already accepted it.
    fn of(root: &Path) -> Result<Self, WatcherError> {
        let reported =
            std::fs::canonicalize(root).map_err(|error| WatcherError::Watch(error.to_string()))?;
        Ok(Self {
            given: root.to_path_buf(),
            reported,
        })
    }

    /// Whether `path` is the watched root itself rather than anything under it.
    fn is_root(&self, path: &Path) -> bool {
        path == self.given
    }

    /// The same file under the given root; a path outside the reported root is kept.
    fn rebase(&self, path: PathBuf) -> PathBuf {
        let rebased = path
            .strip_prefix(&self.reported)
            .ok()
            .map(|rest| self.given.join(rest));
        rebased.unwrap_or(path)
    }
}

/// Stop forwarding when the asynchronous owner releases its receiver.
fn forward_events(
    events: std_mpsc::Receiver<notify::Result<Event>>,
    output: &mpsc::UnboundedSender<ChangeEvent>,
    alias: &RootAlias,
) {
    for change in events
        .into_iter()
        .filter_map(|result| observed_event(result, alias))
    {
        if output.send(change).is_err() {
            return;
        }
    }
}

/// Preserve coarse events and report observation failures without exposing paths.
fn observed_event(result: notify::Result<Event>, alias: &RootAlias) -> Option<ChangeEvent> {
    match result {
        Ok(event) if event.need_rescan() => Some(rescan(event.paths.len())),
        Ok(event) => map_event(event, alias),
        Err(error) => Some(rescan(error.paths.len())),
    }
}

/// Both native rescan flags and observation errors invalidate the complete scope.
fn rescan(affected_paths: usize) -> ChangeEvent {
    tracing::warn!(affected_paths, "Filesystem observation failed");
    ChangeEvent::Rescan
}

/// Converts only filesystem changes that require analyzer work. A touched root is the
/// directory's own metadata, which macOS reports for every child change even when the
/// watch is shallow; each child's own event names what changed, so it carries no work.
/// A removed root is kept: its consumers must learn that their scope is gone.
fn map_event(event: Event, alias: &RootAlias) -> Option<ChangeEvent> {
    let Event { kind, paths, .. } = event;
    let rebased = paths.into_iter().map(|path| alias.rebase(path));
    match kind {
        EventKind::Create(_) | EventKind::Modify(_) => {
            let touched: Vec<PathBuf> = rebased.filter(|path| !alias.is_root(path)).collect();
            (!touched.is_empty()).then_some(ChangeEvent::Touched(touched))
        }
        EventKind::Remove(_) => Some(ChangeEvent::Removed(rebased.collect())),
        _ => None,
    }
}
