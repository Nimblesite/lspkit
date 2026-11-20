//! Integration tests for `lspkit::vfs::text` occurrence scanning and
//! encoding-aware measurement. [VFS-TEXT-OCCURRENCES] [VFS-TEXT-MEASURE]
//! [VFS-TEXT-PREFIX]

use lspkit::vfs::{char_width, measure, occurrences, prefix_to, Position, PositionEncoding, Range};

#[path = "vfs_text_common/mod.rs"]
mod vfs_text_common;

use vfs_text_common::ALL_ENCODINGS;

/// Expected range of one occurrence: (line, start character, end character).
type Span = (u32, u32, u32);

/// One `occurrences` table row.
#[derive(Clone)]
struct ScanCase {
    /// Label used in failure messages.
    label: &'static str,
    /// Whole document text.
    text: &'static str,
    /// Name to scan for.
    name: &'static str,
    /// Encoding of the reported spans.
    encoding: PositionEncoding,
    /// Every whole-word occurrence, in document order.
    expected: &'static [Span],
}

/// ASCII rows whose columns match in every encoding, permuted across all
/// three.
fn ascii_scan_cases() -> Vec<ScanCase> {
    let ascii_doc = "let foo = 1;\nlet foobar = foo + _foo;\nfoo";
    let substring_doc = "foobar _foo foo2 foo.bar foo,foo!";
    let mut cases = Vec::new();
    for encoding in ALL_ENCODINGS {
        cases.extend_from_slice(&[
            ScanCase {
                label: "whole word only across lines",
                text: ascii_doc,
                name: "foo",
                encoding,
                expected: &[(0, 4, 7), (1, 13, 16), (2, 0, 3)],
            },
            ScanCase {
                label: "leading underscore name matches its run",
                text: ascii_doc,
                name: "_foo",
                encoding,
                expected: &[(1, 19, 23)],
            },
            ScanCase {
                label: "name is case sensitive",
                text: ascii_doc,
                name: "FOO",
                encoding,
                expected: &[],
            },
            ScanCase {
                label: "empty name matches nothing",
                text: ascii_doc,
                name: "",
                encoding,
                expected: &[],
            },
            ScanCase {
                label: "name with non-identifier characters matches nothing",
                text: ascii_doc,
                name: "foo +",
                encoding,
                expected: &[],
            },
            ScanCase {
                label: "substring must not match longer runs",
                text: substring_doc,
                name: "foo",
                encoding,
                expected: &[(0, 17, 20), (0, 25, 28), (0, 29, 32)],
            },
            ScanCase {
                label: "empty text matches nothing",
                text: "",
                name: "foo",
                encoding,
                expected: &[],
            },
        ]);
    }
    cases
}

/// Rows whose spans differ per encoding: CJK, emoji, combining marks.
fn script_scan_cases() -> Vec<ScanCase> {
    vec![
        ScanCase {
            label: "cjk name utf-8 spans",
            text: "数量 = 1;\nlet x = 数量;\n数量数量",
            name: "数量",
            encoding: PositionEncoding::Utf8,
            expected: &[(0, 0, 6), (1, 8, 14)],
        },
        ScanCase {
            label: "cjk name utf-16 spans",
            text: "数量 = 1;\nlet x = 数量;\n数量数量",
            name: "数量",
            encoding: PositionEncoding::Utf16,
            expected: &[(0, 0, 2), (1, 8, 10)],
        },
        ScanCase {
            label: "cjk name utf-32 spans",
            text: "数量 = 1;\nlet x = 数量;\n数量数量",
            name: "数量",
            encoding: PositionEncoding::Utf32,
            expected: &[(0, 0, 2), (1, 8, 10)],
        },
        ScanCase {
            label: "emoji separates runs utf-8",
            text: "foo😀foo foo",
            name: "foo",
            encoding: PositionEncoding::Utf8,
            expected: &[(0, 0, 3), (0, 7, 10), (0, 11, 14)],
        },
        ScanCase {
            label: "emoji separates runs utf-16",
            text: "foo😀foo foo",
            name: "foo",
            encoding: PositionEncoding::Utf16,
            expected: &[(0, 0, 3), (0, 5, 8), (0, 9, 12)],
        },
        ScanCase {
            label: "emoji separates runs utf-32",
            text: "foo😀foo foo",
            name: "foo",
            encoding: PositionEncoding::Utf32,
            expected: &[(0, 0, 3), (0, 4, 7), (0, 8, 11)],
        },
        ScanCase {
            label: "combining-mark name matches only equal runs",
            text: "e\u{0301} xe\u{0301}",
            name: "e\u{0301}",
            encoding: PositionEncoding::Utf16,
            expected: &[(0, 0, 2)],
        },
    ]
}

#[test]
fn occurrences_match_table() {
    let mut cases = ascii_scan_cases();
    cases.extend(script_scan_cases());
    for case in cases {
        let found = occurrences(case.text, case.name, case.encoding);
        let expected: Vec<Range> = case
            .expected
            .iter()
            .map(|&(line, start, end)| {
                Range::new(Position::new(line, start), Position::new(line, end))
            })
            .collect();
        let label = format!(
            "{label} (name {name}, {encoding})",
            label = case.label,
            name = case.name,
            encoding = case.encoding
        );
        assert_eq!(found, expected, "spans for {label}");
    }
}

#[test]
fn char_width_matches_table() {
    let table: &[(char, PositionEncoding, u32)] = &[
        ('a', PositionEncoding::Utf8, 1),
        ('a', PositionEncoding::Utf16, 1),
        ('a', PositionEncoding::Utf32, 1),
        ('_', PositionEncoding::Utf8, 1),
        ('_', PositionEncoding::Utf16, 1),
        ('_', PositionEncoding::Utf32, 1),
        ('é', PositionEncoding::Utf8, 2),
        ('é', PositionEncoding::Utf16, 1),
        ('é', PositionEncoding::Utf32, 1),
        ('\u{0301}', PositionEncoding::Utf8, 2),
        ('\u{0301}', PositionEncoding::Utf16, 1),
        ('\u{0301}', PositionEncoding::Utf32, 1),
        ('你', PositionEncoding::Utf8, 3),
        ('你', PositionEncoding::Utf16, 1),
        ('你', PositionEncoding::Utf32, 1),
        ('😀', PositionEncoding::Utf8, 4),
        ('😀', PositionEncoding::Utf16, 2),
        ('😀', PositionEncoding::Utf32, 1),
        ('٣', PositionEncoding::Utf8, 2),
        ('٣', PositionEncoding::Utf16, 1),
        ('٣', PositionEncoding::Utf32, 1),
    ];
    for &(ch, encoding, expected) in table {
        assert_eq!(char_width(ch, encoding), expected, "width of {ch:?}");
    }
}

#[test]
fn measure_matches_table() {
    let table: &[(&str, PositionEncoding, u32)] = &[
        ("", PositionEncoding::Utf8, 0),
        ("", PositionEncoding::Utf16, 0),
        ("", PositionEncoding::Utf32, 0),
        ("let x", PositionEncoding::Utf8, 5),
        ("let x", PositionEncoding::Utf16, 5),
        ("let x", PositionEncoding::Utf32, 5),
        ("😀", PositionEncoding::Utf8, 4),
        ("😀", PositionEncoding::Utf16, 2),
        ("😀", PositionEncoding::Utf32, 1),
        ("a😀你", PositionEncoding::Utf8, 8),
        ("a😀你", PositionEncoding::Utf16, 4),
        ("a😀你", PositionEncoding::Utf32, 3),
        ("e\u{0301}", PositionEncoding::Utf8, 3),
        ("e\u{0301}", PositionEncoding::Utf16, 2),
        ("e\u{0301}", PositionEncoding::Utf32, 2),
    ];
    for &(text, encoding, expected) in table {
        assert_eq!(measure(text, encoding), expected, "measure of {text:?}");
    }
}

#[test]
fn prefix_to_matches_table() {
    let line = "a😀你e";
    let table: &[(&str, u32, PositionEncoding, Option<&str>)] = &[
        (line, 0, PositionEncoding::Utf8, Some("")),
        (line, 0, PositionEncoding::Utf16, Some("")),
        (line, 0, PositionEncoding::Utf32, Some("")),
        (line, 1, PositionEncoding::Utf8, Some("a")),
        (line, 1, PositionEncoding::Utf16, Some("a")),
        (line, 1, PositionEncoding::Utf32, Some("a")),
        (line, 2, PositionEncoding::Utf8, None),
        (line, 2, PositionEncoding::Utf16, None),
        (line, 2, PositionEncoding::Utf32, Some("a😀")),
        (line, 3, PositionEncoding::Utf8, None),
        (line, 3, PositionEncoding::Utf16, Some("a😀")),
        (line, 3, PositionEncoding::Utf32, Some("a😀你")),
        (line, 4, PositionEncoding::Utf8, None),
        (line, 4, PositionEncoding::Utf16, Some("a😀你")),
        (line, 4, PositionEncoding::Utf32, Some("a😀你e")),
        (line, 5, PositionEncoding::Utf8, Some("a😀")),
        (line, 5, PositionEncoding::Utf16, Some("a😀你e")),
        (line, 5, PositionEncoding::Utf32, None),
        (line, 7, PositionEncoding::Utf8, None),
        (line, 8, PositionEncoding::Utf8, Some("a😀你")),
        (line, 9, PositionEncoding::Utf8, Some("a😀你e")),
        (line, 10, PositionEncoding::Utf8, None),
        (line, 6, PositionEncoding::Utf32, None),
    ];
    for &(text, character, encoding, expected) in table {
        assert_eq!(
            prefix_to(text, character, encoding),
            expected,
            "prefix_to {text:?} at {character} ({encoding})"
        );
    }
}

#[test]
fn prefix_to_handles_combining_marks() {
    let line = "e\u{0301}x";
    let table: &[(u32, PositionEncoding, Option<&str>)] = &[
        (1, PositionEncoding::Utf8, Some("e")),
        (2, PositionEncoding::Utf8, None),
        (3, PositionEncoding::Utf8, Some("e\u{0301}")),
        (4, PositionEncoding::Utf8, Some(line)),
        (5, PositionEncoding::Utf8, None),
        (2, PositionEncoding::Utf16, Some("e\u{0301}")),
        (3, PositionEncoding::Utf16, Some(line)),
        (4, PositionEncoding::Utf16, None),
        (2, PositionEncoding::Utf32, Some("e\u{0301}")),
    ];
    for &(character, encoding, expected) in table {
        assert_eq!(
            prefix_to(line, character, encoding),
            expected,
            "prefix_to at {character} ({encoding})"
        );
    }
}
