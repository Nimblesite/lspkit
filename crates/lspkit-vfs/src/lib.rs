//! `lspkit-vfs` — concurrent open-document store for LSP and analysis tools.
//!
//! Tracks open documents by URI with LSP version numbers and applies
//! incremental edits via a rope buffer. The negotiated [`PositionEncoding`]
//! is honored on every edit so positions sent by clients in UTF-16 round-trip
//! correctly against the UTF-8-internal store. Locks never span `.await`.

pub mod document;
pub mod edit;
pub mod encoding;

pub use crate::document::{DocumentUri, DocumentVersion, Vfs, VfsError};
pub use crate::edit::{EditError, Position, Range, TextEdit};
pub use crate::encoding::{PositionEncoding, UnknownEncoding, negotiate};
