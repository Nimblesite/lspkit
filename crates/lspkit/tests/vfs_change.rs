//! Integration tests for `Vfs::change` semantics.

use lspkit::vfs::{
    DocumentUri, DocumentVersion, Position, PositionEncoding, Range, TextEdit, Vfs, VfsError,
};

fn uri(path: &str) -> DocumentUri {
    DocumentUri::new(path)
}

fn make_edit(start: (u32, u32), end: (u32, u32), text: &str) -> TextEdit {
    let range = Range::new(Position::new(start.0, start.1), Position::new(end.0, end.1));
    TextEdit::new(range, text.to_owned())
}

#[test]
fn change_unknown_uri_returns_not_found() {
    let store = Vfs::new(PositionEncoding::Utf16);
    let result = store.change(
        &uri("file:///missing.txt"),
        &[make_edit((0, 0), (0, 0), "x")],
        DocumentVersion::new(2),
    );
    let err = result.expect_err("missing URI must error");
    match err {
        VfsError::NotFound(path) => assert_eq!(path, "file:///missing.txt"),
        other => assert!(
            matches!(other, VfsError::NotFound(_)),
            "unexpected error: {other:?}"
        ),
    }
}

#[test]
fn change_stale_equal_version_is_rejected() {
    let store = Vfs::new(PositionEncoding::Utf16);
    let doc = uri("file:///a.txt");
    store.open(doc.clone(), "abc", DocumentVersion::new(5));
    let result = store.change(
        &doc,
        &[make_edit((0, 0), (0, 0), "x")],
        DocumentVersion::new(5),
    );
    let err = result.expect_err("equal version must error");
    match err {
        VfsError::StaleVersion { have, got } => {
            assert_eq!(have, 5);
            assert_eq!(got, 5);
        }
        other => assert!(
            matches!(other, VfsError::StaleVersion { .. }),
            "unexpected error: {other:?}"
        ),
    }
}

#[test]
fn change_older_version_is_rejected() {
    let store = Vfs::new(PositionEncoding::Utf16);
    let doc = uri("file:///a.txt");
    store.open(doc.clone(), "abc", DocumentVersion::new(10));
    let result = store.change(
        &doc,
        &[make_edit((0, 0), (0, 0), "x")],
        DocumentVersion::new(9),
    );
    let err = result.expect_err("older version must error");
    match err {
        VfsError::StaleVersion { have, got } => {
            assert_eq!(have, 10);
            assert_eq!(got, 9);
        }
        other => assert!(
            matches!(other, VfsError::StaleVersion { .. }),
            "unexpected error: {other:?}"
        ),
    }
}

#[test]
fn change_updates_text_and_version_on_success() {
    let store = Vfs::new(PositionEncoding::Utf8);
    let doc = uri("file:///a.txt");
    store.open(doc.clone(), "abc", DocumentVersion::new(1));
    store
        .change(
            &doc,
            &[make_edit((0, 3), (0, 3), "DEF")],
            DocumentVersion::new(2),
        )
        .expect("append must succeed");

    assert_eq!(store.text(&doc).as_deref(), Some("abcDEF"));
    assert_eq!(store.version(&doc), Some(DocumentVersion::new(2)));
}

#[test]
fn change_applies_multiple_edits_in_order() {
    let store = Vfs::new(PositionEncoding::Utf8);
    let doc = uri("file:///a.txt");
    store.open(doc.clone(), "abcdef", DocumentVersion::new(1));
    let edits = vec![
        make_edit((0, 0), (0, 1), "A"),
        make_edit((0, 5), (0, 6), "F"),
    ];
    store
        .change(&doc, &edits, DocumentVersion::new(2))
        .expect("sequential edits must succeed");
    assert_eq!(store.text(&doc).as_deref(), Some("AbcdeF"));
}

#[test]
fn change_with_empty_edits_only_bumps_version() {
    let store = Vfs::new(PositionEncoding::Utf16);
    let doc = uri("file:///a.txt");
    store.open(doc.clone(), "abc", DocumentVersion::new(1));
    store
        .change(&doc, &[], DocumentVersion::new(4))
        .expect("empty edit list must succeed");
    assert_eq!(store.text(&doc).as_deref(), Some("abc"));
    assert_eq!(store.version(&doc), Some(DocumentVersion::new(4)));
}

#[test]
fn change_propagates_edit_error_for_out_of_bounds_line() {
    let store = Vfs::new(PositionEncoding::Utf8);
    let doc = uri("file:///a.txt");
    store.open(doc.clone(), "abc", DocumentVersion::new(1));
    let result = store.change(
        &doc,
        &[make_edit((50, 0), (50, 0), "x")],
        DocumentVersion::new(2),
    );
    let err = result.expect_err("out-of-bounds line must error");
    match err {
        VfsError::Edit(inner) => {
            let rendered = format!("{inner}");
            assert!(
                rendered.contains("position out of bounds"),
                "got {rendered}"
            );
        }
        other => assert!(
            matches!(other, VfsError::Edit(_)),
            "unexpected error: {other:?}"
        ),
    }
}

#[test]
fn vfs_error_display_renders_not_found() {
    let err = VfsError::NotFound("file:///x".to_owned());
    let rendered = format!("{err}");
    assert!(rendered.contains("file:///x"));
    assert!(rendered.contains("not found"));
}

#[test]
fn vfs_error_display_renders_stale_version() {
    let err = VfsError::StaleVersion { have: 5, got: 4 };
    let rendered = format!("{err}");
    assert!(rendered.contains("have 5"));
    assert!(rendered.contains("got 4"));
}
