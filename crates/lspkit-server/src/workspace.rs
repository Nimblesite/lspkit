//! Multi-root workspace abstraction.
//!
//! Modern LSPs support multi-root workspaces (a single client may open many
//! folders at once). This type tracks the configured roots and exposes
//! folder-scoped iteration so handlers can route queries correctly.

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

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
    /// Shared multi-root configuration replaced and queried across handlers.
    folders: Arc<RwLock<Vec<WorkspaceFolder>>>,
}

impl Workspace {
    /// New empty workspace.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Shared access to the folders; a panicked writer left the list whole.
    fn read(&self) -> RwLockReadGuard<'_, Vec<WorkspaceFolder>> {
        self.folders.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Exclusive access to the folders; a panicked writer left the list whole.
    fn write(&self) -> RwLockWriteGuard<'_, Vec<WorkspaceFolder>> {
        self.folders.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Replace the folder set wholesale (used at `initialize` time).
    pub fn set_folders(&self, folders: Vec<WorkspaceFolder>) {
        *self.write() = folders;
    }

    /// Append a folder.
    pub fn add(&self, folder: WorkspaceFolder) {
        self.write().push(folder);
    }

    /// Remove a folder by root path. [PATH-CENTRAL]
    pub fn remove(&self, root: &Path) {
        self.write()
            .retain(|f| !lspkit::path::same_path(&f.root, root));
    }

    /// Snapshot the current folder list.
    #[must_use]
    pub fn folders(&self) -> Vec<WorkspaceFolder> {
        self.read().clone()
    }

    /// Find the folder containing `path`, if any. [PATH-CENTRAL]
    #[must_use]
    pub fn folder_for(&self, path: &Path) -> Option<WorkspaceFolder> {
        self.read()
            .iter()
            .find(|f| lspkit::path::is_within(path, &f.root))
            .cloned()
    }

    /// Number of configured folders.
    #[must_use]
    pub fn len(&self) -> usize {
        self.read().len()
    }

    /// `true` if no folders are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
