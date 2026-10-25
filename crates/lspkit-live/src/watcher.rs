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
    _inner: RecommendedWatcher,
    rx: mpsc::UnboundedReceiver<ChangeEvent>,
}

/// Owned watcher and shared debounce stream; dropping it releases native watching.
pub struct CoalescedWatcher {
    triggers: mpsc::UnboundedReceiver<crate::scheduler::Trigger>,
    reader: tokio::task::JoinHandle<()>,
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
        let (out_tx, rx) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("lspkit-watch".to_owned())
            .spawn(move || forward_events(event_rx, &out_tx))
            .map_err(|e| WatcherError::Setup(e.to_string()))?;
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

/// Stop forwarding when the asynchronous owner releases its receiver.
fn forward_events(
    events: std_mpsc::Receiver<notify::Result<Event>>,
    output: &mpsc::UnboundedSender<ChangeEvent>,
) {
    for change in events.into_iter().filter_map(observed_event) {
        if output.send(change).is_err() {
            return;
        }
    }
}

/// Preserve coarse events and report observation failures without exposing paths.
fn observed_event(result: notify::Result<Event>) -> Option<ChangeEvent> {
    match result {
        Ok(event) if event.need_rescan() => Some(rescan(event.paths.len())),
        Ok(event) => map_event(event),
        Err(error) => Some(rescan(error.paths.len())),
    }
}

/// Both native rescan flags and observation errors invalidate the complete scope.
fn rescan(affected_paths: usize) -> ChangeEvent {
    tracing::warn!(affected_paths, "Filesystem observation failed");
    ChangeEvent::Rescan
}

fn map_event(event: Event) -> Option<ChangeEvent> {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) => Some(ChangeEvent::Touched(event.paths)),
        EventKind::Remove(_) => Some(ChangeEvent::Removed(event.paths)),
        _ => None,
    }
}
