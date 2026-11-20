//! Integration tests for `lspkit::vfs::text::word_at`. [VFS-TEXT-WORD]
//!
//! Tables permute `PositionEncoding` × script (ASCII, emoji, CJK, combining
//! marks) × cursor placement: start/middle/end of word, between words, empty
//! lines, out-of-range line and character, underscores and digits.

use lspkit::vfs::{word_at, Position, PositionEncoding, Range};

#[path = "vfs_text_common/mod.rs"]
mod vfs_text_common;

use vfs_text_common::ALL_ENCODINGS;

/// Expected word result: (word, line, start character, end character).
type Word = (&'static str, u32, u32, u32);

/// One `word_at` table row.
#[derive(Clone)]
struct WordCase {
    /// Label used in failure messages.
    label: &'static str,
    /// Whole document text.
    text: &'static str,
    /// Cursor (line, character) in `encoding` units.
    cursor: (u32, u32),
    /// Encoding of the cursor and reported span.
    encoding: PositionEncoding,
    /// The identifier under the cursor and its span, or `None`.
    expected: Option<Word>,
}

/// Multi-line ASCII document: `"let alpha = beta2;\ngamma3 delta"`.
const ASCII_DOC: &str = "let alpha = beta2;\ngamma3 delta";

/// Underscore/digit document: runs `foo_bar foo1 _baz ab12cd`.
const SYMBOL_DOC: &str = "foo_bar foo1 _baz ab12cd";

/// CJK document: `"数量 = value"`.
const CJK_DOC: &str = "数量 = value";

/// Emoji document: `"x😀 foo"`.
const EMOJI_DOC: &str = "x😀 foo";

/// Combining-mark document: `"e\u{0301} foo"` (decomposed é).
const COMBINING_DOC: &str = "e\u{0301} foo";

/// Mixed-script identifier document: `"日本語123 is_one"`.
const MIXED_DOC: &str = "日本語123 is_one";

/// Cursor placements over pure-ASCII lines, permuted across encodings.
fn placement_cases() -> Vec<WordCase> {
    let mut cases = Vec::new();
    for encoding in ALL_ENCODINGS {
        cases.extend_from_slice(&[
            WordCase {
                label: "keyword start",
                text: ASCII_DOC,
                cursor: (0, 0),
                encoding,
                expected: Some(("let", 0, 0, 3)),
            },
            WordCase {
                label: "word start boundary",
                text: ASCII_DOC,
                cursor: (0, 4),
                encoding,
                expected: Some(("alpha", 0, 4, 9)),
            },
            WordCase {
                label: "word middle",
                text: ASCII_DOC,
                cursor: (0, 6),
                encoding,
                expected: Some(("alpha", 0, 4, 9)),
            },
            WordCase {
                label: "boundary after word falls back to run",
                text: ASCII_DOC,
                cursor: (0, 9),
                encoding,
                expected: Some(("alpha", 0, 4, 9)),
            },
            WordCase {
                label: "cursor on terminator after identifier",
                text: ASCII_DOC,
                cursor: (0, 17),
                encoding,
                expected: Some(("beta2", 0, 12, 17)),
            },
            WordCase {
                label: "end of line past terminator has no run",
                text: ASCII_DOC,
                cursor: (0, 18),
                encoding,
                expected: None,
            },
            WordCase {
                label: "digit-suffixed identifier",
                text: ASCII_DOC,
                cursor: (0, 15),
                encoding,
                expected: Some(("beta2", 0, 12, 17)),
            },
            WordCase {
                label: "second line between words",
                text: ASCII_DOC,
                cursor: (1, 6),
                encoding,
                expected: Some(("gamma3", 1, 0, 6)),
            },
            WordCase {
                label: "second line word start",
                text: ASCII_DOC,
                cursor: (1, 7),
                encoding,
                expected: Some(("delta", 1, 7, 12)),
            },
            WordCase {
                label: "end of last line",
                text: ASCII_DOC,
                cursor: (1, 12),
                encoding,
                expected: Some(("delta", 1, 7, 12)),
            },
            WordCase {
                label: "out-of-range character",
                text: ASCII_DOC,
                cursor: (0, 19),
                encoding,
                expected: None,
            },
        ]);
    }
    cases
}

/// Document shapes: underscores, digits, empty lines, permuted across
/// encodings.
fn structure_cases() -> Vec<WordCase> {
    let mut cases = Vec::new();
    for encoding in ALL_ENCODINGS {
        cases.extend_from_slice(&[
            WordCase {
                label: "out-of-range line",
                text: ASCII_DOC,
                cursor: (2, 0),
                encoding,
                expected: None,
            },
            WordCase {
                label: "far out-of-range line and character",
                text: ASCII_DOC,
                cursor: (7, 40),
                encoding,
                expected: None,
            },
            WordCase {
                label: "underscore inside identifier",
                text: SYMBOL_DOC,
                cursor: (0, 4),
                encoding,
                expected: Some(("foo_bar", 0, 0, 7)),
            },
            WordCase {
                label: "identifier with trailing digits",
                text: SYMBOL_DOC,
                cursor: (0, 10),
                encoding,
                expected: Some(("foo1", 0, 8, 12)),
            },
            WordCase {
                label: "leading underscore identifier",
                text: SYMBOL_DOC,
                cursor: (0, 13),
                encoding,
                expected: Some(("_baz", 0, 13, 17)),
            },
            WordCase {
                label: "digits inside identifier",
                text: SYMBOL_DOC,
                cursor: (0, 21),
                encoding,
                expected: Some(("ab12cd", 0, 18, 24)),
            },
            WordCase {
                label: "empty line between words",
                text: "foo\n\nbar",
                cursor: (1, 0),
                encoding,
                expected: None,
            },
            WordCase {
                label: "trailing newline empty line",
                text: "foo\nbar\n",
                cursor: (2, 0),
                encoding,
                expected: None,
            },
            WordCase {
                label: "empty document",
                text: "",
                cursor: (0, 0),
                encoding,
                expected: None,
            },
            WordCase {
                label: "carriage return is not an identifier char",
                text: "foo\r\nbar",
                cursor: (0, 3),
                encoding,
                expected: Some(("foo", 0, 0, 3)),
            },
        ]);
    }
    cases
}

/// CJK and mixed-script rows whose spans differ per encoding.
fn script_cases() -> Vec<WordCase> {
    vec![
        WordCase {
            label: "cjk identifier start",
            text: CJK_DOC,
            cursor: (0, 0),
            encoding: PositionEncoding::Utf8,
            expected: Some(("数量", 0, 0, 6)),
        },
        WordCase {
            label: "cjk identifier start",
            text: CJK_DOC,
            cursor: (0, 0),
            encoding: PositionEncoding::Utf16,
            expected: Some(("数量", 0, 0, 2)),
        },
        WordCase {
            label: "cjk identifier start",
            text: CJK_DOC,
            cursor: (0, 0),
            encoding: PositionEncoding::Utf32,
            expected: Some(("数量", 0, 0, 2)),
        },
        WordCase {
            label: "word after cjk prefix",
            text: CJK_DOC,
            cursor: (0, 9),
            encoding: PositionEncoding::Utf8,
            expected: Some(("value", 0, 9, 14)),
        },
        WordCase {
            label: "word after cjk prefix",
            text: CJK_DOC,
            cursor: (0, 5),
            encoding: PositionEncoding::Utf16,
            expected: Some(("value", 0, 5, 10)),
        },
        WordCase {
            label: "cursor inside word after cjk prefix",
            text: CJK_DOC,
            cursor: (0, 7),
            encoding: PositionEncoding::Utf32,
            expected: Some(("value", 0, 5, 10)),
        },
        WordCase {
            label: "cjk with digits one run utf-8",
            text: MIXED_DOC,
            cursor: (0, 0),
            encoding: PositionEncoding::Utf8,
            expected: Some(("日本語123", 0, 0, 12)),
        },
        WordCase {
            label: "cjk with digits one run utf-16",
            text: MIXED_DOC,
            cursor: (0, 3),
            encoding: PositionEncoding::Utf16,
            expected: Some(("日本語123", 0, 0, 6)),
        },
    ]
}

/// Emoji and combining-mark rows whose cursor resolution differs per
/// encoding.
fn mark_cases() -> Vec<WordCase> {
    vec![
        WordCase {
            label: "cursor inside surrogate pair resolves to prior word",
            text: EMOJI_DOC,
            cursor: (0, 2),
            encoding: PositionEncoding::Utf16,
            expected: Some(("x", 0, 0, 1)),
        },
        WordCase {
            label: "cursor inside emoji utf-8 bytes resolves to prior word",
            text: EMOJI_DOC,
            cursor: (0, 3),
            encoding: PositionEncoding::Utf8,
            expected: Some(("x", 0, 0, 1)),
        },
        WordCase {
            label: "utf-32 cursor after emoji lands on space",
            text: EMOJI_DOC,
            cursor: (0, 2),
            encoding: PositionEncoding::Utf32,
            expected: None,
        },
        WordCase {
            label: "word after emoji",
            text: EMOJI_DOC,
            cursor: (0, 6),
            encoding: PositionEncoding::Utf8,
            expected: Some(("foo", 0, 6, 9)),
        },
        WordCase {
            label: "word after emoji",
            text: EMOJI_DOC,
            cursor: (0, 4),
            encoding: PositionEncoding::Utf16,
            expected: Some(("foo", 0, 4, 7)),
        },
        WordCase {
            label: "word after emoji",
            text: EMOJI_DOC,
            cursor: (0, 3),
            encoding: PositionEncoding::Utf32,
            expected: Some(("foo", 0, 3, 6)),
        },
        WordCase {
            label: "combining mark continues an identifier utf-16",
            text: COMBINING_DOC,
            cursor: (0, 1),
            encoding: PositionEncoding::Utf16,
            expected: Some(("e\u{0301}", 0, 0, 2)),
        },
        WordCase {
            label: "combining mark mid-scalar snaps back to run utf-8",
            text: COMBINING_DOC,
            cursor: (0, 2),
            encoding: PositionEncoding::Utf8,
            expected: Some(("e\u{0301}", 0, 0, 3)),
        },
        WordCase {
            label: "combining mark continues an identifier utf-32",
            text: COMBINING_DOC,
            cursor: (0, 1),
            encoding: PositionEncoding::Utf32,
            expected: Some(("e\u{0301}", 0, 0, 2)),
        },
    ]
}

#[test]
fn word_at_matches_table() {
    let mut cases = placement_cases();
    cases.extend(structure_cases());
    cases.extend(script_cases());
    cases.extend(mark_cases());
    for case in cases {
        let found = word_at(
            case.text,
            Position::new(case.cursor.0, case.cursor.1),
            case.encoding,
        );
        let actual = found.as_ref().map(|word| (word.text(), word.range()));
        let expected = case.expected.map(|(text, line, start, end)| {
            (
                text,
                Range::new(Position::new(line, start), Position::new(line, end)),
            )
        });
        let label = format!(
            "{label} ({encoding})",
            label = case.label,
            encoding = case.encoding
        );
        assert_eq!(actual, expected, "word_at for {label}");
    }
}
