//! Shared fixtures for the vfs integration test binaries.

use lspkit::vfs::{
    DocumentUri, DocumentVersion, Position, PositionEncoding, Range, TextEdit, Vfs, VfsError,
};

/// Build a `Vfs` with the given encoding holding one document at `file:///doc.txt`.
pub fn store_with(encoding: PositionEncoding, text: &str) -> (Vfs, DocumentUri) {
    let store = Vfs::new(encoding);
    let doc = DocumentUri::new("file:///doc.txt");
    store.open(doc.clone(), text, DocumentVersion::new(1));
    (store, doc)
}

/// Build a `TextEdit` over the inclusive range `(start, end)` with replacement `text`.
pub fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> TextEdit {
    TextEdit::new(
        Range::new(Position::new(start.0, start.1), Position::new(end.0, end.1)),
        text.to_owned(),
    )
}

/// Apply a single edit, bumping the document to version 2.
pub fn apply_one(store: &Vfs, doc: &DocumentUri, edit: TextEdit) -> Result<(), VfsError> {
    store.change(doc, &[edit], DocumentVersion::new(2))
}
