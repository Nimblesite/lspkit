//! Integration tests for `Vfs` open/close/read/version lifecycle.

use lspkit_vfs::{DocumentUri, DocumentVersion, PositionEncoding, Vfs};

fn uri(path: &str) -> DocumentUri {
    DocumentUri::new(path)
}

#[test]
fn new_vfs_is_empty() {
    let store = Vfs::new(PositionEncoding::Utf16);
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
    assert_eq!(store.encoding(), PositionEncoding::Utf16);
}

#[test]
fn default_vfs_uses_default_encoding() {
    let store = Vfs::default();
    assert!(store.is_empty());
    assert_eq!(store.encoding(), PositionEncoding::default());
}

#[test]
fn open_adds_document_with_text_and_version() {
    let store = Vfs::new(PositionEncoding::Utf8);
    let doc = uri("file:///a.txt");
    store.open(doc.clone(), "hello", DocumentVersion::new(1));

    assert_eq!(store.len(), 1);
    assert!(!store.is_empty());
    assert_eq!(store.text(&doc).as_deref(), Some("hello"));
    assert_eq!(store.version(&doc), Some(DocumentVersion::new(1)));
}

#[test]
fn open_replaces_existing_document() {
    let store = Vfs::new(PositionEncoding::Utf16);
    let doc = uri("file:///a.txt");
    store.open(doc.clone(), "first", DocumentVersion::new(1));
    store.open(doc.clone(), "second", DocumentVersion::new(7));

    assert_eq!(store.len(), 1);
    assert_eq!(store.text(&doc).as_deref(), Some("second"));
    assert_eq!(store.version(&doc), Some(DocumentVersion::new(7)));
}

#[test]
fn close_removes_only_named_document() {
    let store = Vfs::new(PositionEncoding::Utf16);
    let doc_a = uri("file:///a.txt");
    let doc_b = uri("file:///b.txt");
    store.open(doc_a.clone(), "alpha", DocumentVersion::new(1));
    store.open(doc_b.clone(), "beta", DocumentVersion::new(1));

    store.close(&doc_a);

    assert_eq!(store.len(), 1);
    assert!(store.text(&doc_a).is_none());
    assert_eq!(store.text(&doc_b).as_deref(), Some("beta"));
}

#[test]
fn close_unknown_uri_is_noop() {
    let store = Vfs::new(PositionEncoding::Utf16);
    store.close(&uri("file:///never-opened.txt"));
    assert!(store.is_empty());
}

#[test]
fn text_of_unknown_document_is_none() {
    let store = Vfs::new(PositionEncoding::Utf16);
    assert!(store.text(&uri("file:///missing.txt")).is_none());
}

#[test]
fn version_of_unknown_document_is_none() {
    let store = Vfs::new(PositionEncoding::Utf16);
    assert!(store.version(&uri("file:///missing.txt")).is_none());
}

#[test]
fn clone_shares_underlying_store() {
    let store_a = Vfs::new(PositionEncoding::Utf16);
    let store_b = store_a.clone();
    let doc = uri("file:///shared.txt");
    store_a.open(doc.clone(), "shared", DocumentVersion::new(1));

    assert_eq!(store_b.len(), 1);
    assert_eq!(store_b.text(&doc).as_deref(), Some("shared"));
}

#[test]
fn document_uri_round_trips_string() {
    let raw = "file:///path/with/segments.rs";
    let doc = DocumentUri::new(raw);
    assert_eq!(doc.as_str(), raw);
    assert_eq!(doc.clone(), doc);
}

#[test]
fn document_version_get_returns_raw_value() {
    let version = DocumentVersion::new(42);
    assert_eq!(version.get(), 42);
}

#[test]
fn document_version_supports_negative_values() {
    let version = DocumentVersion::new(-3);
    assert_eq!(version.get(), -3);
}

#[test]
fn document_version_orders_numerically() {
    let earlier = DocumentVersion::new(1);
    let later = DocumentVersion::new(2);
    assert!(earlier < later);
}

#[test]
fn vfs_debug_format_is_non_empty() {
    let store = Vfs::new(PositionEncoding::Utf32);
    let rendered = format!("{store:?}");
    assert_ne!(rendered, "");
}
