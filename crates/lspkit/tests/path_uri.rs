//! End-to-end tests for [`lspkit::path`] conversions.

use std::path::Path;
#[cfg(not(windows))]
use std::path::PathBuf;

use lspkit::path::{path_to_uri, uri_to_path, UriError};

/// `path` encodes to exactly `uri`, which decodes back to exactly `path`.
#[cfg(not(windows))]
fn assert_round_trip(path: &Path, uri: &str) {
    assert_eq!(
        path_to_uri(path).ok().as_deref(),
        Some(uri),
        "{}",
        path.display()
    );
    assert_eq!(uri_to_path(uri).ok().as_deref(), Some(path), "{uri}");
}

#[cfg(not(windows))]
#[test]
fn round_trip_simple_unix_path() {
    assert_round_trip(Path::new("/tmp/foo.txt"), "file:///tmp/foo.txt");
}

#[cfg(not(windows))]
#[test]
fn round_trip_path_with_spaces() {
    assert_round_trip(
        Path::new("/tmp/my project/foo bar.txt"),
        "file:///tmp/my%20project/foo%20bar.txt",
    );
}

#[cfg(not(windows))]
#[test]
fn round_trip_path_with_unicode() {
    assert_round_trip(
        Path::new("/tmp/caf\u{00e9}/file.txt"),
        "file:///tmp/caf%C3%A9/file.txt",
    );
}

#[cfg(not(windows))]
#[test]
fn round_trip_path_with_hash_and_question() {
    assert_round_trip(Path::new("/tmp/a#b?c.txt"), "file:///tmp/a%23b%3Fc.txt");
}

#[test]
fn relative_path_rejected() {
    let rejected = path_to_uri(Path::new("relative/foo.txt"));
    assert!(
        matches!(&rejected, Err(UriError::InvalidPath(p)) if p == Path::new("relative/foo.txt")),
        "{rejected:?}"
    );
}

#[test]
fn non_file_scheme_rejected() {
    let rejected = uri_to_path("https://example.com/foo");
    assert!(
        matches!(&rejected, Err(UriError::UnsupportedScheme(s)) if s == "https://example.com/foo"),
        "{rejected:?}"
    );
}

#[test]
fn invalid_percent_encoding_rejected() {
    let rejected = uri_to_path("file:///tmp/%ZZbroken");
    assert!(
        matches!(rejected, Err(UriError::InvalidUtf8(_))),
        "{rejected:?}"
    );
}

/// [URI-WINDOWS] A `%` not followed by two hex digits is malformed on every
/// platform; it is never passed through into the path.
#[test]
fn truncated_percent_encoding_rejected() {
    for uri in ["file:///tmp/a%", "file:///tmp/a%2"] {
        let rejected = uri_to_path(uri);
        assert!(
            matches!(rejected, Err(UriError::InvalidUtf8(None))),
            "{uri}: {rejected:?}"
        );
    }
}

#[cfg(not(windows))]
#[test]
fn lowercase_hex_percent_decodes() {
    assert_eq!(
        uri_to_path("file:///tmp/a%2cb%c3%a9").ok(),
        Some(PathBuf::from("/tmp/a,b\u{e9}"))
    );
}

#[cfg(not(windows))]
#[test]
fn uppercase_hex_percent_decodes() {
    assert_eq!(
        uri_to_path("file:///tmp/a%2Cb%C3%A9").ok(),
        Some(PathBuf::from("/tmp/a,b\u{e9}"))
    );
}

/// [URI-WINDOWS] An encoded `/` stays inside its file name, which no Unix
/// file can hold, so the URI is rejected instead of gaining a folder.
#[cfg(not(windows))]
#[test]
fn encoded_separator_is_rejected_never_split_on() {
    for uri in ["file:///tmp/a%2fb", "file:///tmp/a%2Fb"] {
        let rejected = uri_to_path(uri);
        assert!(
            matches!(&rejected, Err(UriError::NoLocalPath(named)) if named == uri),
            "{uri}: {rejected:?}"
        );
    }
    assert_eq!(
        uri_to_path("file:///tmp/a%5Cb").ok(),
        Some(PathBuf::from(r"/tmp/a\b")),
        "a Unix name may hold a backslash"
    );
    assert_eq!(
        uri_to_path("file:///tmp/a%252Fb").ok(),
        Some(PathBuf::from("/tmp/a%2Fb")),
        "decoded once, never twice"
    );
    let host = uri_to_path("file://host%2Fshare/a.rs");
    assert!(
        matches!(host, Err(UriError::NoLocalPath(_))),
        "an encoded / in the host: {host:?}"
    );
}

#[test]
fn invalid_utf8_after_decode_rejected() {
    // %FF %FE are not valid UTF-8.
    let rejected = uri_to_path("file:///tmp/%FF%FE");
    assert!(
        matches!(rejected, Err(UriError::InvalidUtf8(_))),
        "{rejected:?}"
    );
}

#[test]
fn error_display_lists_offending_input() {
    let rejected = uri_to_path("ftp://x");
    assert!(
        rejected
            .as_ref()
            .is_err_and(|err| err.to_string().contains("ftp://x")),
        "{rejected:?}"
    );
}

#[test]
fn error_debug_works() {
    let rejected = path_to_uri(Path::new("rel"));
    assert!(
        rejected
            .as_ref()
            .is_err_and(|err| format!("{err:?}").contains("InvalidPath")),
        "{rejected:?}"
    );
}

/// [URI-AUTHORITY] A URI's host is its authority, never the first path segment.
#[cfg(not(windows))]
#[test]
fn uri_authority_is_parsed_not_read_as_a_path_segment() {
    let local_forms = [
        "file://localhost/tmp/a.rs",
        "file://LocalHost/tmp/a.rs",
        "file:///tmp/a.rs",
    ];
    for uri in local_forms {
        let decoded = uri_to_path(uri);
        assert_eq!(
            decoded.as_deref().ok(),
            Some(Path::new("/tmp/a.rs")),
            "{uri}"
        );
        assert!(
            decoded.as_ref().is_ok_and(|path| path.is_absolute()),
            "{uri} must decode to an absolute path"
        );
        assert_eq!(
            decoded
                .as_deref()
                .ok()
                .and_then(|path| path_to_uri(path).ok())
                .as_deref(),
            Some("file:///tmp/a.rs"),
            "{uri} re-encodes without its host"
        );
    }
    for uri in ["file://", "file://localhost"] {
        assert_eq!(
            uri_to_path(uri).ok(),
            Some(PathBuf::from("/")),
            "{uri} names the root"
        );
    }
    let host = uri_to_path("file://%ZZhost/a.rs");
    assert!(
        matches!(host, Err(UriError::InvalidUtf8(None))),
        "a malformed host: {host:?}"
    );
    for uri in [
        "file://server/share/a.rs",
        "file://wsl.localhost/Ubuntu/a.rs",
        "file://server",
    ] {
        let rejected = uri_to_path(uri);
        assert!(
            matches!(&rejected, Err(UriError::NoLocalPath(named)) if named == uri),
            "{uri}: {rejected:?}"
        );
        assert!(
            rejected
                .as_ref()
                .is_err_and(|err| err.to_string().contains(uri)),
            "the error names {uri}"
        );
    }
}

/// A file name that is not valid UTF-8 cannot be written into a URI.
#[cfg(not(windows))]
#[test]
fn non_utf8_path_rejected() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let path = PathBuf::from("/tmp").join(OsStr::from_bytes(&[0x66, 0xFF, 0x6F]));
    let rejected = path_to_uri(&path);
    assert!(
        matches!(&rejected, Err(UriError::InvalidPath(named)) if *named == path),
        "{rejected:?}"
    );
}

#[cfg(not(windows))]
#[test]
fn empty_path_after_scheme_decodes_to_root() {
    assert_eq!(uri_to_path("file:///").ok(), Some(PathBuf::from("/")));
}
