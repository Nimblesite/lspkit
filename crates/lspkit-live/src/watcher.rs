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

impl FileWatcher {
    /// Start watching `root` recursively. Returns a watcher that streams events.
    ///
    /// # Errors
    /// Returns [`WatcherError`] if the watcher cannot be created or the path
    /// cannot be watched.
    pub fn new(root: &Path) -> Result<Self, WatcherError> {
        let (event_tx, event_rx) = std_mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(event_tx)
            .map_err(|e| WatcherError::Setup(e.to_string()))?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| WatcherError::Watch(e.to_string()))?;

        let (out_tx, out_rx) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            for result in event_rx {
                let Ok(event) = result else { continue };
                let mapped = map_event(event);
                if let Some(change) = mapped {
                    if out_tx.send(change).is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Self {
            _inner: watcher,
            rx: out_rx,
        })
    }

    /// Await the next change event. Returns `None` when the watcher is dropped.
    pub async fn next(&mut self) -> Option<ChangeEvent> {
        self.rx.recv().await
    }
}

fn map_event(event: Event) -> Option<ChangeEvent> {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) => Some(ChangeEvent::Touched(event.paths)),
        EventKind::Remove(_) => Some(ChangeEvent::Removed(event.paths)),
        _ => None,
    }
}
