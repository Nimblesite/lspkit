//! Incremental edit application.
//!
//! Edits are described as `(range, replacement)` pairs over the negotiated
//! position encoding. Applying an edit converts the range to UTF-8 byte
//! offsets and splices the rope.

use ropey::Rope;

use crate::encoding::PositionEncoding;

/// A zero-based `(line, character)` position in a document.
///
/// The `character` offset is interpreted in the document's negotiated
/// [`PositionEncoding`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based character offset within the line.
    pub character: u32,
}

impl Position {
    /// Construct a position.
    #[must_use]
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// A half-open `[start, end)` range over positions.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Range {
    /// Inclusive start position.
    pub start: Position,
    /// Exclusive end position.
    pub end: Position,
}

impl Range {
    /// Construct a range.
    #[must_use]
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

/// A single incremental edit: replace `range` with `text`.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct TextEdit {
    /// The range to replace.
    pub range: Range,
    /// The replacement text.
    pub text: String,
}

impl TextEdit {
    /// Construct an edit.
    #[must_use]
    pub const fn new(range: Range, text: String) -> Self {
        Self { range, text }
    }
}

/// Errors from applying an edit.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// The edit's position lies outside the document.
    #[error("position out of bounds: line {line}, character {character}")]
    OutOfBounds {
        /// Offending line.
        line: u32,
        /// Offending character offset.
        character: u32,
    },
}

pub(crate) fn apply(
    rope: &mut Rope,
    edits: &[TextEdit],
    encoding: PositionEncoding,
) -> Result<(), EditError> {
    for edit in edits {
        let start = position_to_byte(rope, edit.range.start, encoding)?;
        let end = position_to_byte(rope, edit.range.end, encoding)?;
        let start_char = rope.byte_to_char(start);
        let end_char = rope.byte_to_char(end);
        rope.remove(start_char..end_char);
        rope.insert(start_char, &edit.text);
    }
    Ok(())
}

fn position_to_byte(
    rope: &Rope,
    pos: Position,
    encoding: PositionEncoding,
) -> Result<usize, EditError> {
    let line_idx = pos.line as usize;
    if line_idx > rope.len_lines() {
        return Err(EditError::OutOfBounds { line: pos.line, character: pos.character });
    }
    let line_byte_start = rope.line_to_byte(line_idx.min(rope.len_lines()));
    let line = rope.line(line_idx.min(rope.len_lines().saturating_sub(1)));
    let line_str = line.to_string();
    let char_offset = match encoding {
        PositionEncoding::Utf8 => pos.character as usize,
        PositionEncoding::Utf16 => utf16_to_utf8_offset(&line_str, pos.character as usize)?,
        PositionEncoding::Utf32 => utf32_to_utf8_offset(&line_str, pos.character as usize)?,
    };
    Ok(line_byte_start + char_offset)
}

fn utf16_to_utf8_offset(line: &str, utf16_units: usize) -> Result<usize, EditError> {
    let mut counted = 0usize;
    let mut byte_offset = 0usize;
    for ch in line.chars() {
        if counted >= utf16_units {
            break;
        }
        counted += ch.len_utf16();
        byte_offset += ch.len_utf8();
    }
    if counted < utf16_units {
        return Err(EditError::OutOfBounds {
            line: u32::MAX,
            character: u32::try_from(utf16_units).unwrap_or(u32::MAX),
        });
    }
    Ok(byte_offset)
}

fn utf32_to_utf8_offset(line: &str, code_points: usize) -> Result<usize, EditError> {
    line.char_indices()
        .nth(code_points)
        .map_or_else(
            || {
                if line.chars().count() == code_points {
                    Ok(line.len())
                } else {
                    Err(EditError::OutOfBounds {
                        line: u32::MAX,
                        character: u32::try_from(code_points).unwrap_or(u32::MAX),
                    })
                }
            },
            |(byte_idx, _)| Ok(byte_idx),
        )
}
