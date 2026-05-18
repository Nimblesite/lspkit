//! Integration tests for edits under `PositionEncoding::Utf8`.

use lspkit_vfs::{
    DocumentUri, DocumentVersion, Position, PositionEncoding, Range, TextEdit, Vfs, VfsError,
};

fn store_with(text: &str) -> (Vfs, DocumentUri) {
    let store = Vfs::new(PositionEncoding::Utf8);
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
fn insert_at_start_of_line() {
    let (store, doc) = store_with("hello");
    apply_one(&store, &doc, edit((0, 0), (0, 0), "X")).expect("insert");
    assert_eq!(store.text(&doc).as_deref(), Some("Xhello"));
}

#[test]
fn insert_at_end_of_line() {
    let (store, doc) = store_with("hello");
    apply_one(&store, &doc, edit((0, 5), (0, 5), "!")).expect("append");
    assert_eq!(store.text(&doc).as_deref(), Some("hello!"));
}

#[test]
fn replace_middle_segment() {
    let (store, doc) = store_with("hello");
    apply_one(&store, &doc, edit((0, 1), (0, 4), "EY")).expect("replace");
    assert_eq!(store.text(&doc).as_deref(), Some("hEYo"));
}

#[test]
fn delete_substring_by_replacing_with_empty() {
    let (store, doc) = store_with("hello");
    apply_one(&store, &doc, edit((0, 1), (0, 4), "")).expect("delete");
    assert_eq!(store.text(&doc).as_deref(), Some("ho"));
}

#[test]
fn insert_on_second_line() {
    let (store, doc) = store_with("a\nbc\n");
    apply_one(&store, &doc, edit((1, 1), (1, 1), "Z")).expect("multi-line");
    assert_eq!(store.text(&doc).as_deref(), Some("a\nbZc\n"));
}

#[test]
fn replace_across_lines() {
    let (store, doc) = store_with("abc\ndef\n");
    apply_one(&store, &doc, edit((0, 1), (1, 1), "X")).expect("span");
    assert_eq!(store.text(&doc).as_deref(), Some("aXef\n"));
}

#[test]
fn utf8_character_offset_is_byte_offset_for_multibyte_input() {
    let (store, doc) = store_with("naïve");
    // 'ï' is 2 bytes in UTF-8; replace bytes [2, 4) i.e. the 'ï'.
    apply_one(&store, &doc, edit((0, 2), (0, 4), "i")).expect("replace");
    assert_eq!(store.text(&doc).as_deref(), Some("naive"));
}

#[test]
fn position_past_last_line_returns_out_of_bounds() {
    let (store, doc) = store_with("abc");
    let result = apply_one(&store, &doc, edit((10, 0), (10, 0), "x"));
    let err = result.expect_err("must fail");
    match err {
        VfsError::Edit(_) => {}
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn edit_at_end_of_last_line_is_valid_when_line_count_matches() {
    let (store, doc) = store_with("abc\n");
    // Trailing newline yields two lines per ropey; (1, 0) is start of empty
    // last line and is a valid insertion point.
    apply_one(&store, &doc, edit((1, 0), (1, 0), "Z")).expect("append after newline");
    assert_eq!(store.text(&doc).as_deref(), Some("abc\nZ"));
}

#[test]
fn position_constructors_round_trip_fields() {
    let pos = Position::new(3, 7);
    assert_eq!(pos.line, 3);
    assert_eq!(pos.character, 7);
    let range = Range::new(pos, Position::new(3, 9));
    assert_eq!(range.start, pos);
    assert_eq!(range.end.character, 9);
    let edit = TextEdit::new(range, "x".to_owned());
    assert_eq!(edit.range, range);
    assert_eq!(edit.text, "x");
}
