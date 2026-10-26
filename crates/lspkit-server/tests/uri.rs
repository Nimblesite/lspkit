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

#[test]
fn truncated_percent_encoding_passes_through_bytes() {
    // Only two-char-followed percents are decoded; a stray `%` near end falls through.
    let decoded = uri_to_path("file:///tmp/a%").expect("decoded");
    assert!(decoded.to_string_lossy().contains('%'));
}

#[cfg(not(windows))]
#[test]
fn lowercase_hex_percent_decodes() {
    let decoded = uri_to_path("file:///tmp/a%2fb").expect("decoded");
    assert_eq!(decoded, PathBuf::from("/tmp/a/b"));
}

#[cfg(not(windows))]
#[test]
fn uppercase_hex_percent_decodes() {
    let decoded = uri_to_path("file:///tmp/a%2Fb").expect("decoded");
    assert_eq!(decoded, PathBuf::from("/tmp/a/b"));
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

#[cfg(not(windows))]
#[test]
fn empty_path_after_scheme_decodes_to_root() {
    let decoded = uri_to_path("file:///").expect("decoded");
    assert_eq!(decoded, PathBuf::from("/"));
}
