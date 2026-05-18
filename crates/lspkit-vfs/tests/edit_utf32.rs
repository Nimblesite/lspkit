//! Integration tests for UTF-32 (code-point) position handling.

use lspkit_vfs::{
    DocumentUri, DocumentVersion, Position, PositionEncoding, Range, TextEdit, Vfs, VfsError,
};

fn store_with(text: &str) -> (Vfs, DocumentUri) {
    let store = Vfs::new(PositionEncoding::Utf32);
    let doc = DocumentUri::new("file:///doc.txt");
    store.open(doc.clone(), text, DocumentVersion::new(1));
    (store, doc)
}

fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> TextEdit {
    TextEdit::new(
        Range::new(Position::new(start.0, start.1), Position::new(end.0, end.1)),
        text.to_owned(),
    )
}

fn apply_one(store: &Vfs, doc: &DocumentUri, edit: TextEdit) -> Result<(), VfsError> {
    store.change(doc, &[edit], DocumentVersion::new(2))
}

#[test]
fn ascii_offsets_match_byte_offsets() {
    let (store, doc) = store_with("hello");
    apply_one(&store, &doc, edit((0, 1), (0, 4), "ELL")).expect("replace");
    assert_eq!(store.text(&doc).as_deref(), Some("hELLo"));
}

#[test]
fn each_scalar_counts_as_one_code_point() {
    // "naïve" has 5 code points regardless of UTF-8/UTF-16 width.
    let (store, doc) = store_with("naïve");
    apply_one(&store, &doc, edit((0, 2), (0, 3), "i")).expect("replace ï");
    assert_eq!(store.text(&doc).as_deref(), Some("naive"));
}

#[test]
fn supplementary_plane_scalar_is_one_code_point() {
    // U+1F600 is one code point in UTF-32 (vs two in UTF-16).
    let (store, doc) = store_with("a\u{1F600}b");
    apply_one(&store, &doc, edit((0, 1), (0, 2), "X")).expect("replace emoji");
    assert_eq!(store.text(&doc).as_deref(), Some("aXb"));
}

#[test]
fn insert_at_end_of_line_via_code_point_count() {
    let (store, doc) = store_with("naïve");
    apply_one(&store, &doc, edit((0, 5), (0, 5), "!")).expect("append");
    assert_eq!(store.text(&doc).as_deref(), Some("naïve!"));
}

#[test]
fn insert_at_end_of_emoji_line() {
    let (store, doc) = store_with("a\u{1F600}b");
    apply_one(&store, &doc, edit((0, 3), (0, 3), "!")).expect("append");
    assert_eq!(store.text(&doc).as_deref(), Some("a\u{1F600}b!"));
}

#[test]
fn out_of_bounds_code_point_offset_errors() {
    let (store, doc) = store_with("ab");
    let result = apply_one(&store, &doc, edit((0, 99), (0, 99), "x"));
    let err = result.expect_err("must fail");
    match err {
        VfsError::Edit(inner) => {
            assert!(format!("{inner}").contains("position out of bounds"));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn round_trip_emoji_insert_in_utf32_mode() {
    let (store, doc) = store_with("hi");
    apply_one(&store, &doc, edit((0, 2), (0, 2), "\u{1F600}")).expect("append emoji");
    assert_eq!(store.text(&doc).as_deref(), Some("hi\u{1F600}"));
}

#[test]
fn replace_across_lines_in_utf32() {
    let (store, doc) = store_with("naïve\ncafé\n");
    apply_one(&store, &doc, edit((0, 2), (1, 3), "_")).expect("span");
    assert_eq!(store.text(&doc).as_deref(), Some("na_é\n"));
}
