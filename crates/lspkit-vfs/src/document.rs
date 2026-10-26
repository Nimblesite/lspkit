//! Concurrent open-document store.
//!
//! Tracks open documents by URI with LSP version numbers. Edits are applied
//! through [`crate::edit`] using the negotiated position encoding. Locks are
//! always scoped inside synchronous blocks — no guard ever crosses an
//! `await` boundary.

use std::sync::Arc;

use dashmap::DashMap;
use ropey::Rope;

use crate::edit::{self, EditError, TextEdit};
use crate::encoding::PositionEncoding;

/// Opaque URI identifying a document. Generic over the wire form so this
/// crate does not leak `lsp_types` into its public surface.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocumentUri(String);

impl DocumentUri {
    /// Wrap a string-form URI.
    #[must_use]
    pub fn new(uri: impl Into<String>) -> Self {
        Self(uri.into())
    }

    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// LSP document version. Bumped by the client on every `didChange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentVersion(i32);

impl DocumentVersion {
    /// Construct a version.
    #[must_use]
    pub const fn new(value: i32) -> Self {
        Self(value)
    }

    /// Raw integer value.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

/// Errors from VFS operations.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    /// No document tracked for the given URI.
    #[error("document not found: {0}")]
    NotFound(String),
    /// Incoming edit applied to stale version.
    #[error("stale version: have {have}, got {got}")]
    StaleVersion {
        /// Version currently stored.
        have: i32,
        /// Version supplied by the caller.
        got: i32,
    },
    /// An edit could not be applied.
    #[error(transparent)]
    Edit(#[from] EditError),
}

/// Versioned rope retained for one open document.
#[derive(Debug)]
struct DocumentEntry {
    /// Incrementally editable document contents.
    rope: Rope,
    /// Last client version accepted for this document.
    version: DocumentVersion,
}

/// Concurrent open-document store.
#[derive(Debug, Default, Clone)]
pub struct Vfs {
    /// Open entries shared by cheap clones of the store.
    docs: Arc<DashMap<DocumentUri, DocumentEntry>>,
    /// Encoding used to translate every incoming edit position.
    encoding: PositionEncoding,
}

impl Vfs {
    /// New empty VFS using the given position encoding.
    #[must_use]
    pub fn new(encoding: PositionEncoding) -> Self {
        Self {
            docs: Arc::new(DashMap::new()),
            encoding,
        }
    }

    /// Insert (or replace) a document. Called from `textDocument/didOpen`.
    pub fn open(&self, uri: DocumentUri, text: &str, version: DocumentVersion) {
        let entry = DocumentEntry {
            rope: Rope::from_str(text),
            version,
        };
        // A reopened URI intentionally replaces and drops its previous entry.
        let _ = self.docs.insert(uri, entry);
    }

    /// Apply incremental edits. Called from `textDocument/didChange`.
    ///
    /// # Errors
    /// Returns [`VfsError::NotFound`] if the document is unknown,
    /// [`VfsError::StaleVersion`] if `version` is not strictly greater than
    /// the stored version, or [`VfsError::Edit`] if an edit's range is invalid.
    pub fn change(
        &self,
        uri: &DocumentUri,
        edits: &[TextEdit],
        version: DocumentVersion,
    ) -> Result<(), VfsError> {
        let mut entry = self
            .docs
            .get_mut(uri)
            .ok_or_else(|| VfsError::NotFound(uri.as_str().to_owned()))?;
        if version.get() <= entry.version.get() {
            return Err(VfsError::StaleVersion {
                have: entry.version.get(),
                got: version.get(),
            });
        }
        edit::apply(&mut entry.rope, edits, self.encoding)?;
        entry.version = version;
        Ok(())
    }

    /// Remove a document. Called from `textDocument/didClose`.
    pub fn close(&self, uri: &DocumentUri) {
        // Closing an unknown document is idempotent; a removed entry can be dropped.
        let _ = self.docs.remove(uri);
    }

    /// Read a document's current text. Returns `None` if not tracked.
    #[must_use]
    pub fn text(&self, uri: &DocumentUri) -> Option<String> {
        self.docs.get(uri).map(|entry| entry.rope.to_string())
    }

    /// Read a document's current LSP version. Returns `None` if not tracked.
    #[must_use]
    pub fn version(&self, uri: &DocumentUri) -> Option<DocumentVersion> {
        self.docs.get(uri).map(|entry| entry.version)
    }

    /// The negotiated position encoding.
    #[must_use]
    pub fn encoding(&self) -> PositionEncoding {
        self.encoding
    }

    /// Number of currently tracked documents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// `true` if no documents are tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}
