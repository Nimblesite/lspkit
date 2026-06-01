//! Multi-root workspace abstraction.
//!
//! Modern LSPs support multi-root workspaces (a single client may open many
//! folders at once). This type tracks the configured roots and exposes
//! folder-scoped iteration so handlers can route queries correctly.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// A named workspace folder.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkspaceFolder {
    /// Absolute root path.
    pub root: PathBuf,
    /// Human-readable name supplied by the client.
    pub name: String,
}

impl WorkspaceFolder {
    /// Construct a folder.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, name: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            name: name.into(),
        }
    }
}

/// Multi-root workspace state.
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    folders: Arc<RwLock<Vec<WorkspaceFolder>>>,
}

impl Workspace {
    /// New empty workspace.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the folder set wholesale (used at `initialize` time).
    pub fn set_folders(&self, folders: Vec<WorkspaceFolder>) {
        if let Ok(mut guard) = self.folders.write() {
            *guard = folders;
        }
    }

    /// Append a folder.
    pub fn add(&self, folder: WorkspaceFolder) {
        if let Ok(mut guard) = self.folders.write() {
            guard.push(folder);
        }
    }

    /// Remove a folder by root path.
    pub fn remove(&self, root: &Path) {
        if let Ok(mut guard) = self.folders.write() {
            guard.retain(|f| f.root != root);
        }
    }

    /// Snapshot the current folder list.
    #[must_use]
    pub fn folders(&self) -> Vec<WorkspaceFolder> {
        self.folders
            .read()
            .map_or_else(|_| Vec::new(), |g| g.clone())
    }

    /// Find the folder containing `path`, if any.
    #[must_use]
    pub fn folder_for(&self, path: &Path) -> Option<WorkspaceFolder> {
        self.folders
            .read()
            .ok()
            .and_then(|guard| guard.iter().find(|f| path.starts_with(&f.root)).cloned())
    }

    /// Number of configured folders.
    #[must_use]
    pub fn len(&self) -> usize {
        self.folders.read().map_or(0, |g| g.len())
    }

    /// `true` if no folders are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
