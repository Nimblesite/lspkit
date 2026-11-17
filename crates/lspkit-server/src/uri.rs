//! URI ↔ `PathBuf` conversion: the one place lspkit turns file paths into URIs
//! and back.
//!
//! Handles `file://`, URI authorities, percent-decoding, Windows drive letters,
//! UNC paths and extended-length prefixes ([URI-AUTHORITY], [URI-WINDOWS]).

use std::path::{Path, PathBuf};

/// Errors from URI conversion.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum UriError {
    /// URI scheme is not `file://`.
    #[error("unsupported URI scheme: {0}")]
    UnsupportedScheme(String),
    /// Percent-decoded URI is not valid UTF-8, or holds a malformed `%` escape.
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
/// Domain Windows uses to spell an IPv6 literal as a UNC server name.
const IPV6_LITERAL_SUFFIX: &str = ".ipv6-literal.net";
/// Characters a UNC server name cannot hold, including a URI's userinfo and port.
const HOST_FORBIDDEN: [char; 12] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '@', '[', ']'];
/// Device names Win32 resolves in place of a file whose name starts with one.
const DOS_DEVICES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

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
/// names a UNC path on Windows. On Windows the drive letter is upper case,
/// the server name lower case, and the `\\?\` form is returned only when
/// Win32 would otherwise open a different file ([URI-WINDOWS]).
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
    let (authority, path) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    let host = percent_decode(authority)?;
    let local = host.is_empty() || host.eq_ignore_ascii_case(LOCAL_HOST);
    let no_path = || UriError::NoLocalPath(uri.to_owned());
    let segments = decode_segments(path, style)?.ok_or_else(no_path)?;
    match (style, local) {
        (Style::Unix, true) => Ok(format!("/{}", segments.join("/"))),
        (Style::Unix, false) => Err(no_path()),
        (Style::Windows, _) => WindowsPath::from_uri((!local).then_some(host.as_str()), segments)
            .map(|parsed| parsed.native())
            .ok_or_else(no_path),
    }
}

/// Decodes each path segment on its own, so an encoded `%2F` (or `%5C` on
/// Windows) stays inside its name; `None` when a decoded name holds one.
fn decode_segments(path: &str, style: Style) -> Result<Option<Vec<String>>, UriError> {
    let segments = path
        .split('/')
        .skip(1)
        .map(percent_decode)
        .collect::<Result<Vec<_>, _>>()?;
    let split = segments
        .iter()
        .any(|segment| segment.contains(style.separators()));
    Ok((!split).then_some(segments))
}

impl Style {
    /// Characters that separate path segments, so never appear inside one.
    const fn separators(self) -> &'static [char] {
        match self {
            Self::Unix => &['/'],
            Self::Windows => &['/', '\\'],
        }
    }
}

/// Convert an absolute filesystem path to a `file://` URI ([URI-AUTHORITY]).
///
/// A Windows UNC path keeps its server as the URI authority, and the
/// extended-length prefixes `\\?\` and `\\?\UNC\` are removed. One file has
/// one URI: drive letters are upper case, server names lower case, and
/// repeated separators collapse ([URI-WINDOWS]).
///
/// # Errors
/// Returns [`UriError::InvalidPath`] if the path is not absolute, is a
/// Windows device path (`\\.\`, `\\?\Volume{…}`), or contains components
/// that cannot be expressed in a URI.
pub fn path_to_uri(path: &Path) -> Result<String, UriError> {
    Some(path)
        .filter(|path| path.is_absolute())
        .and_then(Path::to_str)
        .and_then(|text| native_to_uri(text, NATIVE))
        .ok_or_else(|| UriError::InvalidPath(path.to_path_buf()))
}

/// Renders an absolute path written in `style` as a `file://` URI, or `None`
/// when it names no file a URI can reach.
fn native_to_uri(path: &str, style: Style) -> Option<String> {
    match style {
        Style::Unix => Some(format!("{FILE_SCHEME}{}", percent_encode(path))),
        Style::Windows => WindowsPath::from_native(path).map(|parsed| parsed.uri()),
    }
}

/// A native Windows path split on both separators.
fn native_parts(path: &str) -> impl Iterator<Item = String> + '_ {
    path.split(['\\', '/']).map(str::to_owned)
}

/// Where an absolute Windows path is rooted.
enum Root {
    /// `C:\`; the letter is upper case.
    Drive(char),
    /// `\\server\share`; the server name is lower case.
    Unc {
        /// Server name.
        host: String,
        /// Share name.
        share: String,
    },
}

/// An absolute Windows path with no empty, `.` or `..` segments: the single
/// form both conversion directions parse into and render from.
struct WindowsPath {
    /// Drive or network root.
    root: Root,
    /// Folder and file names below the root.
    segments: Vec<String>,
    /// Whether the path ends in a separator (`C:\repo\`).
    trailing: bool,
}

impl WindowsPath {
    /// Parses a URI's decoded path segments; `host` is `None` for the local machine.
    /// `file:////server/share` (an empty authority before a UNC path) is accepted.
    fn from_uri(host: Option<&str>, segments: Vec<String>) -> Option<Self> {
        let mut parts = segments.into_iter();
        match host {
            Some(host) => Self::unc(host, parts),
            None => match parts.next()? {
                first if first.is_empty() => {
                    let host = parts.next()?;
                    Self::unc(&host, parts)
                }
                first => Self::drive(&first, parts),
            },
        }
    }

    /// Parses an absolute Windows path, removing an extended-length prefix.
    /// Device paths (`\\.\`, `\\?\Volume{…}`) name no file and give `None`.
    fn from_native(path: &str) -> Option<Self> {
        if let Some(unc) = path.strip_prefix(VERBATIM_UNC_PREFIX) {
            let mut parts = native_parts(unc);
            let host = parts.next()?;
            return Self::unc(&host, parts);
        }
        let (verbatim, body) = match path.strip_prefix(VERBATIM_PREFIX) {
            Some(drive) => (true, drive),
            None => (false, path),
        };
        let mut parts = native_parts(body);
        let first = parts.next()?;
        if verbatim || !first.is_empty() {
            return Self::drive(&first, parts);
        }
        Self::unc_after_root(parts)
    }

    /// A leading separator must be the pair that opens `\\server\share`.
    fn unc_after_root(mut parts: impl Iterator<Item = String>) -> Option<Self> {
        let host = parts
            .next()
            .filter(String::is_empty)
            .and_then(|_| parts.next())?;
        Self::unc(&host, parts)
    }

    /// `C:` followed by at least one separator.
    fn drive(first: &str, rest: impl Iterator<Item = String>) -> Option<Self> {
        let letter = match first.as_bytes() {
            [letter, b':'] if letter.is_ascii_alphabetic() => {
                char::from(letter.to_ascii_uppercase())
            }
            _ => return None,
        };
        let mut rest = rest.peekable();
        rest.peek()
            .is_some()
            .then(|| Self::below(Root::Drive(letter), rest))
    }

    /// `\\server\share`, with a reachable server name and a non-empty share.
    fn unc(host: &str, mut rest: impl Iterator<Item = String>) -> Option<Self> {
        let host = unc_host(host)?;
        let share = rest.next().filter(|share| !share.is_empty())?;
        Some(Self::below(Root::Unc { host, share }, rest))
    }

    /// Resolves `.`, `..` and empty segments lexically, as Win32 does; `..`
    /// never climbs above the root.
    fn below(root: Root, raw: impl Iterator<Item = String>) -> Self {
        let mut segments = Vec::new();
        let mut trailing = false;
        for segment in raw {
            trailing = segment.is_empty();
            match segment.as_str() {
                "" | "." => {}
                ".." => segments.truncate(segments.len().saturating_sub(1)),
                _ => segments.push(segment),
            }
        }
        Self {
            root,
            segments,
            trailing,
        }
    }

    /// The native path, extended-length only when Win32 would otherwise
    /// rewrite a segment into a different file.
    fn native(&self) -> String {
        let verbatim = self.segments.iter().any(|segment| win32_rewrites(segment));
        let head = match (&self.root, verbatim) {
            (Root::Drive(letter), false) => format!(r"{letter}:\"),
            (Root::Drive(letter), true) => format!(r"{VERBATIM_PREFIX}{letter}:\"),
            (Root::Unc { host, share }, false) => format!(r"{UNC_PREFIX}{host}\{share}"),
            (Root::Unc { host, share }, true) => format!(r"{VERBATIM_UNC_PREFIX}{host}\{share}"),
        };
        self.render(head, '\\', str::to_owned)
    }

    /// The canonical `file://` URI.
    fn uri(&self) -> String {
        let head = match &self.root {
            Root::Drive(letter) => format!("{FILE_SCHEME}/{letter}:/"),
            Root::Unc { host, share } => format!(
                "{FILE_SCHEME}{}/{}",
                percent_encode(host),
                percent_encode(share)
            ),
        };
        self.render(head, '/', percent_encode)
    }

    /// Writes `head`, then each segment through `spell`, separated by `separator`.
    fn render(&self, head: String, separator: char, spell: impl Fn(&str) -> String) -> String {
        let mut out = head;
        for segment in &self.segments {
            if !out.ends_with(separator) {
                out.push(separator);
            }
            out.push_str(&spell(segment));
        }
        if self.trailing && !out.ends_with(separator) {
            out.push(separator);
        }
        out
    }
}

/// The lower-case UNC server name for a URI host or path server, with an
/// IPv6 literal `[fe80::1]` spelled `fe80--1.ipv6-literal.net`. Device
/// namespaces (`.`, `?`), ports and userinfo name no server.
fn unc_host(raw: &str) -> Option<String> {
    let host = match raw
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
    {
        Some(ipv6) => format!(
            "{}{IPV6_LITERAL_SUFFIX}",
            ipv6.replace(':', "-").replace('%', "s")
        ),
        None => raw.to_owned(),
    };
    let reachable = !matches!(host.as_str(), "" | "." | "?") && !host.contains(HOST_FORBIDDEN);
    reachable.then(|| host.to_ascii_lowercase())
}

/// Whether Win32 opens something other than `segment` when it is written
/// without `\\?\`: a trailing dot or space is dropped, and a DOS device name
/// opens the device.
fn win32_rewrites(segment: &str) -> bool {
    let stem = match segment.split_once('.') {
        Some((stem, _extension)) => stem,
        None => segment,
    }
    .trim_end_matches(' ');
    segment.ends_with(['.', ' '])
        || DOS_DEVICES
            .iter()
            .any(|device| device.eq_ignore_ascii_case(stem))
}

/// Decodes `%XX` escapes; a `%` not followed by two hex digits is an error.
fn percent_decode(input: &str) -> Result<String, UriError> {
    let mut out = Vec::with_capacity(input.len());
    let mut rest = input.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        rest = match (byte, tail) {
            (b'%', [hi, lo, after @ ..]) => {
                out.push((hex_digit(*hi)? << 4) | hex_digit(*lo)?);
                after
            }
            (b'%', _) => return Err(UriError::InvalidUtf8(None)),
            _ => {
                out.push(byte);
                tail
            }
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
#[path = "uri_style_tests.rs"]
mod style_tests;
