//! End-to-end tests for [`lspkit_server::uri`] conversions.

use std::path::PathBuf;

use lspkit_server::{path_to_uri, uri_to_path, UriError};

#[cfg(not(windows))]
#[test]
fn round_trip_simple_unix_path() {
    let path = PathBuf::from("/tmp/foo.txt");
    let uri = path_to_uri(&path).expect("encoded");
    assert_eq!(uri, "file:///tmp/foo.txt");
    let decoded = uri_to_path(&uri).expect("decoded");
    assert_eq!(decoded, path);
}

#[cfg(not(windows))]
#[test]
fn round_trip_path_with_spaces() {
    let path = PathBuf::from("/tmp/my project/foo bar.txt");
    let uri = path_to_uri(&path).expect("encoded");
    assert!(uri.contains("%20"));
    let decoded = uri_to_path(&uri).expect("decoded");
    assert_eq!(decoded, path);
}

#[cfg(not(windows))]
#[test]
fn round_trip_path_with_unicode() {
    let path = PathBuf::from("/tmp/caf\u{00e9}/file.txt");
    let uri = path_to_uri(&path).expect("encoded");
    let decoded = uri_to_path(&uri).expect("decoded");
    assert_eq!(decoded, path);
}

#[cfg(not(windows))]
#[test]
fn round_trip_path_with_hash_and_question() {
    let path = PathBuf::from("/tmp/a#b?c.txt");
    let uri = path_to_uri(&path).expect("encoded");
    assert!(uri.contains("%23"));
    assert!(uri.contains("%3F"));
    let decoded = uri_to_path(&uri).expect("decoded");
    assert_eq!(decoded, path);
}

#[test]
fn relative_path_rejected() {
    let err = path_to_uri(&PathBuf::from("relative/foo.txt")).expect_err("must reject");
    match err {
        UriError::InvalidPath(p) => assert_eq!(p, PathBuf::from("relative/foo.txt")),
        other => assert!(
            matches!(other, UriError::InvalidPath(_)),
            "wrong variant: {other:?}"
        ),
    }
}

#[test]
fn non_file_scheme_rejected() {
    let err = uri_to_path("https://example.com/foo").expect_err("must reject");
    match err {
        UriError::UnsupportedScheme(s) => assert_eq!(s, "https://example.com/foo"),
        other => assert!(
            matches!(other, UriError::UnsupportedScheme(_)),
            "wrong variant: {other:?}"
        ),
    }
}

#[test]
fn invalid_percent_encoding_rejected() {
    let err = uri_to_path("file:///tmp/%ZZbroken").expect_err("must reject");
    match err {
        UriError::InvalidUtf8(_) => {}
        other => assert!(
            matches!(other, UriError::InvalidUtf8(_)),
            "wrong variant: {other:?}"
        ),
    }
}

/// [URI-WINDOWS] A `%` not followed by two hex digits is malformed on every
/// platform; it is never passed through into the path.
#[test]
fn truncated_percent_encoding_rejected() {
    for uri in ["file:///tmp/a%", "file:///tmp/a%2"] {
        let err = uri_to_path(uri).expect_err("must reject");
        assert!(matches!(err, UriError::InvalidUtf8(None)), "{uri}: {err:?}");
    }
}

#[cfg(not(windows))]
#[test]
fn lowercase_hex_percent_decodes() {
    let decoded = uri_to_path("file:///tmp/a%2cb%c3%a9").expect("decoded");
    assert_eq!(decoded, PathBuf::from("/tmp/a,b\u{e9}"));
}

#[cfg(not(windows))]
#[test]
fn uppercase_hex_percent_decodes() {
    let decoded = uri_to_path("file:///tmp/a%2Cb%C3%A9").expect("decoded");
    assert_eq!(decoded, PathBuf::from("/tmp/a,b\u{e9}"));
}

/// [URI-WINDOWS] An encoded `/` stays inside its file name, which no Unix
/// file can hold, so the URI is rejected instead of gaining a folder.
#[cfg(not(windows))]
#[test]
fn encoded_separator_is_rejected_never_split_on() {
    for uri in ["file:///tmp/a%2fb", "file:///tmp/a%2Fb"] {
        let err = uri_to_path(uri).expect_err("an encoded separator");
        assert!(
            matches!(&err, UriError::NoLocalPath(rejected) if rejected == uri),
            "{uri}: {err:?}"
        );
    }
    let backslash = uri_to_path("file:///tmp/a%5Cb").expect("a Unix name may hold a backslash");
    assert_eq!(backslash, PathBuf::from(r"/tmp/a\b"));
    let literal = uri_to_path("file:///tmp/a%252Fb").expect("an encoded percent");
    assert_eq!(
        literal,
        PathBuf::from("/tmp/a%2Fb"),
        "decoded once, never twice"
    );
    let host = uri_to_path("file://host%2Fshare/a.rs").expect_err("an encoded / in the host");
    assert!(matches!(host, UriError::NoLocalPath(_)), "{host:?}");
}

#[test]
fn invalid_utf8_after_decode_rejected() {
    // %FF %FE are not valid UTF-8.
    let err = uri_to_path("file:///tmp/%FF%FE").expect_err("must reject");
    match err {
        UriError::InvalidUtf8(_) => {}
        other => assert!(
            matches!(other, UriError::InvalidUtf8(_)),
            "wrong variant: {other:?}"
        ),
    }
}

#[test]
fn error_display_lists_offending_input() {
    let err = uri_to_path("ftp://x").expect_err("must reject");
    let rendered = format!("{err}");
    assert!(rendered.contains("ftp://x"));
}

#[test]
fn error_debug_works() {
    let err = path_to_uri(&PathBuf::from("rel")).expect_err("must reject");
    let rendered = format!("{err:?}");
    assert!(rendered.contains("InvalidPath"));
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
        let decoded = uri_to_path(uri).expect("a local URI decodes");
        assert_eq!(decoded, PathBuf::from("/tmp/a.rs"), "{uri}");
        assert!(
            decoded.is_absolute(),
            "{uri} must decode to an absolute path"
        );
        assert_eq!(
            path_to_uri(&decoded).expect("encoded"),
            "file:///tmp/a.rs",
            "{uri} re-encodes without its host"
        );
    }
    for uri in ["file://", "file://localhost"] {
        let decoded = uri_to_path(uri).expect("a URI with no path names the root");
        assert_eq!(decoded, PathBuf::from("/"), "{uri}");
    }
    let err = uri_to_path("file://%ZZhost/a.rs").expect_err("a malformed host");
    assert!(matches!(err, UriError::InvalidUtf8(None)), "{err:?}");
    for uri in [
        "file://server/share/a.rs",
        "file://wsl.localhost/Ubuntu/a.rs",
        "file://server",
    ] {
        let err = uri_to_path(uri).expect_err("a remote host has no local path here");
        assert!(
            matches!(&err, UriError::NoLocalPath(rejected) if rejected == uri),
            "{uri}: {err:?}"
        );
        assert!(format!("{err}").contains(uri), "the error names {uri}");
    }
}

/// A file name that is not valid UTF-8 cannot be written into a URI.
#[cfg(not(windows))]
#[test]
fn non_utf8_path_rejected() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let path = PathBuf::from("/tmp").join(OsStr::from_bytes(&[0x66, 0xFF, 0x6F]));
    let err = path_to_uri(&path).expect_err("must reject");
    assert!(
        matches!(&err, UriError::InvalidPath(rejected) if *rejected == path),
        "{err:?}"
    );
}

#[cfg(not(windows))]
#[test]
fn empty_path_after_scheme_decodes_to_root() {
    let decoded = uri_to_path("file:///").expect("decoded");
    assert_eq!(decoded, PathBuf::from("/"));
}
