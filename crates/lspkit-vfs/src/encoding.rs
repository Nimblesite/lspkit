//! Position encoding negotiation.
//!
//! LSP 3.17 lets the server negotiate UTF-8, UTF-16, or UTF-32 character
//! offsets. Internally we always store text as UTF-8; this module exposes the
//! negotiated encoding so position math can be performed correctly.

use std::fmt;
use std::str::FromStr;

/// The character offset encoding negotiated with the client.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PositionEncoding {
    /// UTF-8 byte offsets.
    Utf8,
    /// UTF-16 code-unit offsets. LSP's historical default.
    #[default]
    Utf16,
    /// UTF-32 code-point offsets.
    Utf32,
}

impl PositionEncoding {
    /// Wire-format identifier as used in LSP `positionEncoding`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf16 => "utf-16",
            Self::Utf32 => "utf-32",
        }
    }
}

impl fmt::Display for PositionEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PositionEncoding {
    type Err = UnknownEncoding;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "utf-8" => Ok(Self::Utf8),
            "utf-16" => Ok(Self::Utf16),
            "utf-32" => Ok(Self::Utf32),
            other => Err(UnknownEncoding(other.to_owned())),
        }
    }
}

/// Error returned when an encoding string is not recognized.
#[derive(Debug, thiserror::Error)]
#[error("unknown position encoding: {0}")]
pub struct UnknownEncoding(pub String);

/// Pick a server-preferred encoding from a list of client-supported encodings.
///
/// Preference order matches the LSP 3.17 specification: the server SHOULD
/// honor the first encoding the client lists that the server also supports.
/// When `client_supported` is empty, returns [`PositionEncoding::Utf16`].
#[must_use]
pub fn negotiate(client_supported: &[PositionEncoding]) -> PositionEncoding {
    client_supported
        .first()
        .copied()
        .unwrap_or(PositionEncoding::Utf16)
}
