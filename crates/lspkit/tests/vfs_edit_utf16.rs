//! Integration tests for UTF-16 position handling, including surrogate pairs.

use lspkit::vfs::{DocumentUri, EditError, PositionEncoding, Vfs, VfsError};

#[path = "vfs_common/mod.rs"]
mod common;
use common::{apply_one, edit};

fn store_with(text: &str) -> (Vfs, DocumentUri) {
    common::store_with(PositionEncoding::Utf16, text)
}

#[test]
fn ascii_offsets_match_utf8_offsets() {
    let (store, doc) = store_with("hello");
    apply_one(&store, &doc, edit((0, 1), (0, 4), "ELL")).expect("replace");
    assert_eq!(store.text(&doc).as_deref(), Some("hELLo"));
}

#[test]
fn replace_after_two_byte_scalar_uses_utf16_code_units() {
    let (store, doc) = store_with("naïve");
    // In UTF-16, "naïve" is 5 code units: n(1) a(1) ï(1) v(1) e(1).
    // Replace the 'v' (units [3,4)).
    apply_one(&store, &doc, edit((0, 3), (0, 4), "V")).expect("replace");
    assert_eq!(store.text(&doc).as_deref(), Some("naïVe"));
}

#[test]
fn insert_immediately_after_two_byte_scalar() {
    let (store, doc) = store_with("naïve");
    // Insert at UTF-16 unit index 3 (between 'ï' and 'v').
    apply_one(&store, &doc, edit((0, 3), (0, 3), "-")).expect("insert");
    assert_eq!(store.text(&doc).as_deref(), Some("naï-ve"));
}

#[test]
fn replace_three_byte_scalar() {
    // "café日本" - the kanji "日" and "本" each are 1 UTF-16 unit, 3 UTF-8 bytes.
    let (store, doc) = store_with("café日本");
    // UTF-16 layout: c(0) a(1) f(2) é(3) 日(4) 本(5). Replace "日本" (units [4,6)).
    apply_one(&store, &doc, edit((0, 4), (0, 6), "JP")).expect("replace kanji");
    assert_eq!(store.text(&doc).as_deref(), Some("caféJP"));
}

#[test]
fn surrogate_pair_counts_as_two_code_units() {
    // U+1F600 (GRINNING FACE) is encoded as a surrogate pair in UTF-16: 2 code units.
    let (store, doc) = store_with("a\u{1F600}b");
    // UTF-16: a(0) [hi-surrogate](1) [lo-surrogate](2) b(3).
    // Replace exactly the surrogate pair, units [1, 3).
    apply_one(&store, &doc, edit((0, 1), (0, 3), "X")).expect("replace emoji");
    assert_eq!(store.text(&doc).as_deref(), Some("aXb"));
}

#[test]
fn insert_before_surrogate_pair_does_not_split_it() {
    let (store, doc) = store_with("a\u{1F600}b");
    apply_one(&store, &doc, edit((0, 1), (0, 1), "Z")).expect("insert");
    assert_eq!(store.text(&doc).as_deref(), Some("aZ\u{1F600}b"));
}

#[test]
fn insert_after_surrogate_pair_lands_at_correct_byte() {
    let (store, doc) = store_with("a\u{1F600}b");
    // After the pair, UTF-16 index is 3 (before 'b').
    apply_one(&store, &doc, edit((0, 3), (0, 3), "Z")).expect("insert");
    assert_eq!(store.text(&doc).as_deref(), Some("a\u{1F600}Zb"));
}

#[test]
fn out_of_bounds_utf16_offset_returns_edit_error() {
    let (store, doc) = store_with("ab");
    let result = apply_one(&store, &doc, edit((0, 50), (0, 50), "x"));
    let err = result.expect_err("must fail");
    match err {
        VfsError::Edit(inner) => {
            assert!(format!("{inner}").contains("position out of bounds"));
        }
        other => assert!(matches!(other, VfsError::Edit(_)), "unexpected: {other:?}"),
    }
}

#[test]
fn round_trip_after_emoji_insert_preserves_bytes() {
    let (store, doc) = store_with("hi");
    apply_one(&store, &doc, edit((0, 2), (0, 2), "\u{1F600}")).expect("append emoji");
    assert_eq!(store.text(&doc).as_deref(), Some("hi\u{1F600}"));
}

#[test]
fn utf16_offset_at_line_end_is_valid() {
    let (store, doc) = store_with("naïve");
    // End of "naïve" is 5 UTF-16 units; an insertion there must succeed.
    apply_one(&store, &doc, edit((0, 5), (0, 5), "!")).expect("append");
    assert_eq!(store.text(&doc).as_deref(), Some("naïve!"));
}

#[test]
fn utf16_character_past_line_reports_requested_offset() {
    let (store, doc) = store_with("naïve");
    // Unit 9 lies past the line's 5 UTF-16 units; the error must carry the
    // requested unit count, not a conversion failure.
    let result = apply_one(&store, &doc, edit((0, 9), (0, 9), "x"));
    let error = result.expect_err("unit past line end must fail");
    let inner = match &error {
        VfsError::Edit(inner) => Some(inner),
        _ => None,
    }
    .expect("unit past line end must surface as VfsError::Edit");
    assert_eq!(
        inner,
        &EditError::OutOfBounds {
            line: u32::MAX,
            character: 9
        }
    );
    assert_eq!(store.text(&doc).as_deref(), Some("naïve"));
}
