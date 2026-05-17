//! URI ↔ `PathBuf` conversion.
//!
//! Handles `file://`, percent-decoding, Windows drive letters, and UNC paths.

use std::path::{Path, PathBuf};

/// Errors from URI conversion.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum UriError {
    /// URI scheme is not `file://`.
    #[error("unsupported URI scheme: {0}")]
    UnsupportedScheme(String),
    /// Percent-decoded URI is not valid UTF-8.
    #[error("URI is not valid UTF-8 after percent decoding")]
    InvalidUtf8,
    /// Path could not be converted to a URI.
    #[error("path cannot be expressed as a file URI: {0}")]
    InvalidPath(PathBuf),
}

/// Convert a `file://` URI to a `PathBuf`.
///
/// # Errors
/// Returns [`UriError`] for non-`file` schemes or invalid percent encoding.
pub fn uri_to_path(uri: &str) -> Result<PathBuf, UriError> {
    let rest = uri
        .strip_prefix("file://")
        .ok_or_else(|| UriError::UnsupportedScheme(uri.to_owned()))?;
    let trimmed = rest.strip_prefix('/').unwrap_or(rest);
    let decoded = percent_decode(trimmed)?;
    if cfg!(windows) {
        let normalized = decoded.replace('/', "\\");
        Ok(PathBuf::from(normalized))
    } else {
        Ok(PathBuf::from(format!("/{decoded}")))
    }
}

/// Convert a filesystem path to a `file://` URI.
///
/// # Errors
/// Returns [`UriError::InvalidPath`] if the path is not absolute or contains
/// components that cannot be expressed in a URI.
pub fn path_to_uri(path: &Path) -> Result<String, UriError> {
    if !path.is_absolute() {
        return Err(UriError::InvalidPath(path.to_path_buf()));
    }
    let as_str = path.to_str().ok_or_else(|| UriError::InvalidPath(path.to_path_buf()))?;
    let normalized = as_str.replace('\\', "/");
    let encoded = percent_encode(&normalized);
    if normalized.starts_with('/') {
        Ok(format!("file://{encoded}"))
    } else {
        Ok(format!("file:///{encoded}"))
    }
}

fn percent_decode(input: &str) -> Result<String, UriError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' && idx + 2 < bytes.len() {
            let hi = hex_digit(bytes[idx + 1])?;
            let lo = hex_digit(bytes[idx + 2])?;
            out.push((hi << 4) | lo);
            idx += 3;
        } else {
            out.push(bytes[idx]);
            idx += 1;
        }
    }
    String::from_utf8(out).map_err(|_| UriError::InvalidUtf8)
}

fn hex_digit(byte: u8) -> Result<u8, UriError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(UriError::InvalidUtf8),
    }
}

fn percent_encode(input: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'/'
            | b':' => out.push(byte as char),
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}
