//! URI ↔ `PathBuf` conversion.
//!
//! Handles `file://`, URI authorities, percent-decoding, Windows drive letters,
//! UNC paths and extended-length prefixes ([URI-AUTHORITY]).

use std::path::{Path, PathBuf};

/// Errors from URI conversion.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum UriError {
    /// URI scheme is not `file://`.
    #[error("unsupported URI scheme: {0}")]
    UnsupportedScheme(String),
    /// Percent-decoded URI is not valid UTF-8, retaining conversion failures.
    #[error("URI is not valid UTF-8 after percent decoding")]
    InvalidUtf8(#[source] Option<std::string::FromUtf8Error>),
    /// Path could not be converted to a URI.
    #[error("path cannot be expressed as a file URI: {0}")]
    InvalidPath(PathBuf),
    /// URI names a host this platform cannot reach as a path, or no absolute path.
    #[error("URI names no absolute local path: {0}")]
    NoLocalPath(String),
}

/// The `file` scheme prefix every accepted URI carries.
const FILE_SCHEME: &str = "file://";
/// The authority that names the local machine.
const LOCAL_HOST: &str = "localhost";
/// Windows prefix of a network (UNC) path.
const UNC_PREFIX: &str = r"\\";
/// Windows extended-length prefix of a network path.
const VERBATIM_UNC_PREFIX: &str = r"\\?\UNC\";
/// Windows extended-length prefix of a drive path.
const VERBATIM_PREFIX: &str = r"\\?\";

/// Path syntax of the platform a conversion targets.
#[derive(Clone, Copy, Debug)]
enum Style {
    /// `/`-rooted paths; only the local host is reachable.
    Unix,
    /// Drive-letter and UNC paths.
    Windows,
}

/// The style of the platform this crate was built for.
const NATIVE: Style = if cfg!(windows) {
    Style::Windows
} else {
    Style::Unix
};

/// Convert a `file://` URI to an absolute `PathBuf` ([URI-AUTHORITY]).
///
/// An empty or `localhost` authority names a local path; any other host
/// names a UNC path on Windows.
///
/// # Errors
/// Returns [`UriError`] for non-`file` schemes, invalid percent encoding, or
/// a URI that names no absolute path reachable on this platform.
pub fn uri_to_path(uri: &str) -> Result<PathBuf, UriError> {
    uri_to_native(uri, NATIVE).map(PathBuf::from)
}

/// Splits the authority from the path and renders the path for `style`.
fn uri_to_native(uri: &str, style: Style) -> Result<String, UriError> {
    let rest = uri
        .strip_prefix(FILE_SCHEME)
        .ok_or_else(|| UriError::UnsupportedScheme(uri.to_owned()))?;
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let host = percent_decode(authority)?;
    let path = percent_decode(path)?;
    let local = host.is_empty() || host.eq_ignore_ascii_case(LOCAL_HOST);
    match (style, local) {
        (Style::Unix, true) if path.is_empty() => Ok("/".to_owned()),
        (Style::Unix, true) => Ok(path),
        (Style::Windows, true) => windows_drive_path(uri, &path),
        (Style::Windows, false) if path.trim_matches('/').is_empty() => {
            Err(UriError::NoLocalPath(uri.to_owned()))
        }
        (Style::Windows, false) => Ok(format!("{UNC_PREFIX}{host}{}", path.replace('/', "\\"))),
        (Style::Unix, false) => Err(UriError::NoLocalPath(uri.to_owned())),
    }
}

/// `/C:/repo/a.rs` → `C:\repo\a.rs`; anything without a drive root is rejected.
fn windows_drive_path(uri: &str, path: &str) -> Result<String, UriError> {
    let native = path.strip_prefix('/').unwrap_or(path).replace('/', "\\");
    match native.as_bytes() {
        [letter, b':', b'\\', ..] if letter.is_ascii_alphabetic() => Ok(native),
        _ => Err(UriError::NoLocalPath(uri.to_owned())),
    }
}

/// Convert an absolute filesystem path to a `file://` URI ([URI-AUTHORITY]).
///
/// A Windows UNC path keeps its server as the URI authority, and the
/// extended-length prefixes `\\?\` and `\\?\UNC\` are removed.
///
/// # Errors
/// Returns [`UriError::InvalidPath`] if the path is not absolute or contains
/// components that cannot be expressed in a URI.
pub fn path_to_uri(path: &Path) -> Result<String, UriError> {
    if !path.is_absolute() {
        return Err(UriError::InvalidPath(path.to_path_buf()));
    }
    path.to_str()
        .map(|text| native_to_uri(text, NATIVE))
        .ok_or_else(|| UriError::InvalidPath(path.to_path_buf()))
}

/// Renders an absolute path written in `style` as a `file://` URI.
fn native_to_uri(path: &str, style: Style) -> String {
    match style {
        Style::Unix => format!("{FILE_SCHEME}{}", percent_encode(path)),
        Style::Windows => windows_to_uri(&without_verbatim(path).replace('\\', "/")),
    }
}

/// `//server/share/a.rs` → `file://server/share/a.rs`; `C:/a.rs` → `file:///C:/a.rs`.
fn windows_to_uri(forward: &str) -> String {
    match forward.strip_prefix("//") {
        Some(unc) => {
            let (host, path) = unc.split_at(unc.find('/').unwrap_or(unc.len()));
            format!(
                "{FILE_SCHEME}{}{}",
                percent_encode(host),
                percent_encode(path)
            )
        }
        None => format!("{FILE_SCHEME}/{}", percent_encode(forward)),
    }
}

/// Removes the extended-length prefix `std::fs::canonicalize` adds on Windows.
fn without_verbatim(path: &str) -> String {
    path.strip_prefix(VERBATIM_UNC_PREFIX)
        .map(|unc| format!("{UNC_PREFIX}{unc}"))
        .or_else(|| path.strip_prefix(VERBATIM_PREFIX).map(str::to_owned))
        .unwrap_or_else(|| path.to_owned())
}

/// Decodes complete percent triplets and preserves incomplete trailing input.
fn percent_decode(input: &str) -> Result<String, UriError> {
    let mut out = Vec::with_capacity(input.len());
    let mut rest = input.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        rest = if let (b'%', [hi, lo, after @ ..]) = (byte, tail) {
            out.push((hex_digit(*hi)? << 4) | hex_digit(*lo)?);
            after
        } else {
            out.push(byte);
            tail
        };
    }
    String::from_utf8(out).map_err(|source| UriError::InvalidUtf8(Some(source)))
}

/// Converts one ASCII hexadecimal digit into its numeric nibble.
fn hex_digit(byte: u8) -> Result<u8, UriError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(UriError::InvalidUtf8(None)),
    }
}

/// Percent-encodes non-ASCII and reserved bytes while retaining URI separators.
fn percent_encode(input: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(char::from(byte));
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "uri_windows_tests.rs"]
mod windows_tests;
