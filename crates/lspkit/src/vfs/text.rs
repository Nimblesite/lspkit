//! Position-aware text primitives. [VFS-TEXT]
//!
//! Generic, language-agnostic helpers that operate on plain text plus the
//! negotiated [`PositionEncoding`]: the identifier run under a cursor, every
//! whole-word occurrence of a name, and encoding-aware column measurement.
//! Character classification follows Unicode `XID_Continue` — no regexes.

use crate::vfs::edit::utf32_to_utf8_offset;
use crate::vfs::edit::{Position, Range};
use crate::vfs::encoding::PositionEncoding;

/// An identifier run under a cursor plus its location. [VFS-TEXT-WORD]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSpan {
    /// The matched identifier text.
    text: String,
    /// Half-open `[start, end)` range in the negotiated encoding.
    range: Range,
}

impl WordSpan {
    /// The identifier text of this span.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The span's location in the document.
    #[must_use]
    pub fn range(&self) -> Range {
        self.range
    }
}

/// An identifier character: Unicode `XID_Continue`, which covers letters,
/// digits, `_` and combining marks. [VFS-TEXT-IDENTIFIER]
fn is_identifier_char(ch: char) -> bool {
    unicode_ident::is_xid_continue(ch)
}

/// A non-empty run of identifier characters. [VFS-TEXT-IDENTIFIER]
fn is_identifier(name: &str) -> bool {
    !name.is_empty() && name.chars().all(is_identifier_char)
}

/// Largest scalar UTF-8 encodes in one byte.
const UTF8_ONE_BYTE_MAX: u32 = 0x7F;
/// Largest scalar UTF-8 encodes in two bytes.
const UTF8_TWO_BYTE_MAX: u32 = 0x7FF;
/// Largest Basic Multilingual Plane scalar: three UTF-8 bytes, one UTF-16 unit.
const BMP_MAX: u32 = 0xFFFF;

/// Position units one scalar occupies in `encoding`. [VFS-TEXT-MEASURE]
#[must_use]
pub fn char_width(ch: char, encoding: PositionEncoding) -> u32 {
    let scalar = u32::from(ch);
    match encoding {
        PositionEncoding::Utf8 => utf8_width(scalar),
        PositionEncoding::Utf16 if scalar > BMP_MAX => 2,
        PositionEncoding::Utf16 | PositionEncoding::Utf32 => 1,
    }
}

/// UTF-8 bytes of a scalar value.
const fn utf8_width(scalar: u32) -> u32 {
    match scalar {
        s if s <= UTF8_ONE_BYTE_MAX => 1,
        s if s <= UTF8_TWO_BYTE_MAX => 2,
        s if s <= BMP_MAX => 3,
        _ => 4,
    }
}

/// Encoded width of `text`: the saturating sum of its char widths.
/// [VFS-TEXT-MEASURE]
#[must_use]
pub fn measure(text: &str, encoding: PositionEncoding) -> u32 {
    text.chars().fold(0u32, |total, ch| {
        total.saturating_add(char_width(ch, encoding))
    })
}

/// The `number`-th line of `text`, split on `\n`. `None` when out of range.
fn line_at(text: &str, line: u32) -> Option<&str> {
    let index = usize::try_from(line).ok()?;
    text.split('\n').nth(index)
}

/// One maximal identifier run: `(start char, end char, start units, end units)`.
type Run = (usize, usize, u32, u32);

/// Each char with its index and encoded span start and width.
fn encoded_chars(
    line: &str,
    encoding: PositionEncoding,
) -> impl Iterator<Item = (usize, u32, u32, char)> + '_ {
    let mut index = 0usize;
    let mut units = 0u32;
    line.chars().map(move |ch| {
        let width = char_width(ch, encoding);
        let entry = (index, units, width, ch);
        index += 1;
        units = units.saturating_add(width);
        entry
    })
}

/// Every maximal identifier run on a line, with encoded unit spans.
/// [VFS-TEXT-IDENTIFIER]
fn identifier_runs(line: &str, encoding: PositionEncoding) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut open: Option<Run> = None;
    for (index, start_units, width, ch) in encoded_chars(line, encoding) {
        let end_units = start_units.saturating_add(width);
        open = match (open, is_identifier_char(ch)) {
            (None, true) => Some((index, index + 1, start_units, end_units)),
            (Some((start, _, run_start, _)), true) => {
                Some((start, index + 1, run_start, end_units))
            }
            (Some(run), false) => {
                runs.push(run);
                None
            }
            (None, false) => None,
        };
    }
    runs.extend(open);
    runs
}

/// The UTF-8 text of one run, or `None` on an impossible index.
fn run_text<'a>(line: &'a str, run: &Run) -> Option<&'a str> {
    let start = utf32_to_utf8_offset(line, run.0).ok()?;
    let end = utf32_to_utf8_offset(line, run.1).ok()?;
    line.get(start..end)
}

/// The identifier run at a position. [VFS-TEXT-WORD]
///
/// A cursor on an identifier character selects its run; a cursor on any other
/// character, or at end of line, selects the run ending exactly at that
/// boundary. `None` when the column is beyond the line's encoded width or no
/// run qualifies. A column inside a multi-unit character resolves to that
/// character.
#[must_use]
pub fn word_at(text: &str, position: Position, encoding: PositionEncoding) -> Option<WordSpan> {
    let line = line_at(text, position.line)?;
    let cursor = char_index_at(line, position.character, encoding)?;
    let run = run_at_cursor(&identifier_runs(line, encoding), cursor)?;
    let range = Range::new(
        Position::new(position.line, run.2),
        Position::new(position.line, run.3),
    );
    Some(WordSpan {
        text: run_text(line, &run)?.to_owned(),
        range,
    })
}

/// The identifier run selected by a cursor already resolved to a char index.
/// [VFS-TEXT-WORD]
fn run_at_cursor(runs: &[Run], cursor: usize) -> Option<Run> {
    runs.iter()
        .copied()
        .find(|&(start, end, _, _)| start <= cursor && cursor < end)
        .or_else(|| {
            runs.iter()
                .copied()
                .rev()
                .find(|&(_, end, _, _)| end == cursor)
        })
}

/// Resolve an encoded column to a char index on `line`.
///
/// A column inside a multi-unit character resolves to that character; the
/// end-of-line boundary resolves to the char count. `None` beyond the line.
fn char_index_at(line: &str, character: u32, encoding: PositionEncoding) -> Option<usize> {
    encoded_chars(line, encoding)
        .find(|&(_, start, width, _)| start.saturating_add(width) > character)
        .map(|(index, ..)| index)
        .or_else(|| (measure(line, encoding) == character).then(|| line.chars().count()))
}

/// Every whole-word occurrence of `name`, in document order. [VFS-TEXT-OCCURRENCES]
///
/// Only whole identifier runs match, so `foo` never matches inside `foobar`,
/// `_foo` or `foo2`. A `name` that is not a valid identifier yields no
/// occurrences. Matching is case-sensitive and never spans lines.
#[must_use]
pub fn occurrences(text: &str, name: &str, encoding: PositionEncoding) -> Vec<Range> {
    if !is_identifier(name) {
        return Vec::new();
    }
    (0_u32..)
        .zip(text.split('\n'))
        .flat_map(|(line_number, line)| line_occurrences(line, name, encoding, line_number))
        .collect()
}

/// Ranges of whole-word matches of `name` on one line. [VFS-TEXT-OCCURRENCES]
fn line_occurrences(
    line: &str,
    name: &str,
    encoding: PositionEncoding,
    line_number: u32,
) -> Vec<Range> {
    identifier_runs(line, encoding)
        .into_iter()
        .filter(|run| run_text(line, run).is_some_and(|text| text == name))
        .map(|(_, _, start, end)| {
            Range::new(
                Position::new(line_number, start),
                Position::new(line_number, end),
            )
        })
        .collect()
}

/// The line's prefix up to an encoded column boundary. [VFS-TEXT-PREFIX]
///
/// `None` when `character` splits a multi-unit character or lies beyond the
/// line's encoded width.
#[must_use]
pub fn prefix_to(line: &str, character: u32, encoding: PositionEncoding) -> Option<&str> {
    line.get(..prefix_byte_at(line, character, encoding)?)
}

/// UTF-8 byte offset of an exact encoded-column boundary. [VFS-TEXT-PREFIX]
fn prefix_byte_at(line: &str, character: u32, encoding: PositionEncoding) -> Option<usize> {
    encoded_chars(line, encoding)
        .find(|&(_, start_units, _, _)| start_units == character)
        .and_then(|(index, _, _, _)| utf32_to_utf8_offset(line, index).ok())
        .or_else(|| whole_line_boundary(line, character, encoding))
}

/// The whole-line boundary when `character` equals the line's encoded width.
fn whole_line_boundary(line: &str, character: u32, encoding: PositionEncoding) -> Option<usize> {
    (measure(line, encoding) == character).then_some(line.len())
}
