//! Integration tests for `lspkit_vfs::encoding`.

use std::str::FromStr;

use lspkit_vfs::{negotiate, PositionEncoding, UnknownEncoding};

#[test]
fn default_encoding_is_utf16() {
    let default_encoding = PositionEncoding::default();
    assert_eq!(default_encoding, PositionEncoding::Utf16);
}

#[test]
fn as_str_round_trip_utf8() {
    assert_eq!(PositionEncoding::Utf8.as_str(), "utf-8");
}

#[test]
fn as_str_round_trip_utf16() {
    assert_eq!(PositionEncoding::Utf16.as_str(), "utf-16");
}

#[test]
fn as_str_round_trip_utf32() {
    assert_eq!(PositionEncoding::Utf32.as_str(), "utf-32");
}

#[test]
fn display_matches_as_str_for_each_variant() {
    for encoding in [
        PositionEncoding::Utf8,
        PositionEncoding::Utf16,
        PositionEncoding::Utf32,
    ] {
        assert_eq!(format!("{encoding}"), encoding.as_str());
    }
}

#[test]
fn from_str_parses_utf8() {
    let parsed = PositionEncoding::from_str("utf-8").expect("utf-8 must parse");
    assert_eq!(parsed, PositionEncoding::Utf8);
}

#[test]
fn from_str_parses_utf16() {
    let parsed = PositionEncoding::from_str("utf-16").expect("utf-16 must parse");
    assert_eq!(parsed, PositionEncoding::Utf16);
}

#[test]
fn from_str_parses_utf32() {
    let parsed = PositionEncoding::from_str("utf-32").expect("utf-32 must parse");
    assert_eq!(parsed, PositionEncoding::Utf32);
}

#[test]
fn from_str_rejects_unknown_value() {
    let error = PositionEncoding::from_str("utf-7").expect_err("utf-7 must fail");
    let UnknownEncoding(raw) = &error;
    assert_eq!(raw, "utf-7");
    let rendered = format!("{error}");
    assert!(rendered.contains("utf-7"), "render was {rendered}");
}

#[test]
fn from_str_rejects_empty_string() {
    let error = PositionEncoding::from_str("").expect_err("empty must fail");
    assert_eq!(error.0, "");
}

#[test]
fn negotiate_with_empty_returns_utf16() {
    assert_eq!(negotiate(&[]), PositionEncoding::Utf16);
}

#[test]
fn negotiate_returns_first_client_entry_utf8() {
    let chosen = negotiate(&[PositionEncoding::Utf8, PositionEncoding::Utf16]);
    assert_eq!(chosen, PositionEncoding::Utf8);
}

#[test]
fn negotiate_returns_first_client_entry_utf32() {
    let chosen = negotiate(&[PositionEncoding::Utf32, PositionEncoding::Utf8]);
    assert_eq!(chosen, PositionEncoding::Utf32);
}

#[test]
fn encoding_is_copy_and_hashable() {
    use std::collections::HashSet;
    let mut set: HashSet<PositionEncoding> = HashSet::new();
    assert!(set.insert(PositionEncoding::Utf8));
    assert!(!set.insert(PositionEncoding::Utf8));
    assert!(set.insert(PositionEncoding::Utf16));
    assert!(set.insert(PositionEncoding::Utf32));
    assert_eq!(set.len(), 3);
}
