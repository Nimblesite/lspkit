//! [URI-AUTHORITY] [URI-WINDOWS] Path-style rules, isolated so the Linux CI
//! runs the Windows rules too: the public functions only take the Windows
//! branch on a Windows build, where `tests/uri_windows.rs` runs the same table.

use super::{native_to_uri, uri_to_native, Style, UriError};

#[path = "uri_cases.rs"]
mod cases;

#[test]
fn windows_uris_decode_to_exact_paths_and_round_trip() {
    for (uri, path, canonical) in cases::ROUND_TRIPS {
        let decoded = uri_to_native(uri, Style::Windows).expect("a Windows URI decodes");
        assert_eq!(decoded, path, "{uri}");
        assert_eq!(
            native_to_uri(&decoded, Style::Windows).as_deref(),
            Some(canonical),
            "{path}"
        );
        let again = uri_to_native(canonical, Style::Windows).expect("re-decodes");
        assert_eq!(again, path, "{canonical} round-trips");
    }
}

#[test]
fn windows_paths_encode_to_one_canonical_uri() {
    for (path, uri) in cases::PATHS_TO_URIS {
        assert_eq!(
            native_to_uri(path, Style::Windows).as_deref(),
            Some(uri),
            "{path}"
        );
        let decoded = uri_to_native(uri, Style::Windows).expect("decodes");
        assert!(
            !decoded.contains('/'),
            "{uri} decodes with native separators"
        );
    }
}

#[test]
fn windows_device_and_rootless_paths_have_no_uri() {
    for path in cases::PATHS_REJECTED {
        assert_eq!(native_to_uri(path, Style::Windows), None, "{path}");
    }
}

#[test]
fn windows_uris_without_an_absolute_path_are_rejected() {
    for (uri, variant) in cases::URIS_REJECTED {
        let err = uri_to_native(uri, Style::Windows).expect_err("no absolute path");
        assert!(format!("{err:?}").starts_with(variant), "{uri}: {err:?}");
        if let UriError::NoLocalPath(rejected) = &err {
            assert_eq!(rejected, uri, "the error names the URI");
        }
    }
}

/// URIs and the Unix path each names, so this binary's copy of the shared
/// conversion is exercised on both styles, not only the Windows branch.
const UNIX_ROUND_TRIPS: [(&str, &str); 3] = [
    ("file://localhost/tmp/a.rs", "/tmp/a.rs"),
    ("file:///tmp/my%20repo/a.rs", "/tmp/my repo/a.rs"),
    ("file://", "/"),
];

#[test]
fn both_styles_share_one_authority_parser() {
    for (uri, path) in UNIX_ROUND_TRIPS {
        assert_eq!(
            uri_to_native(uri, Style::Unix).expect("local"),
            path,
            "{uri}"
        );
    }
    assert_eq!(
        native_to_uri("/tmp/a b.rs", Style::Unix).as_deref(),
        Some("file:///tmp/a%20b.rs")
    );
    for style in [Style::Unix, Style::Windows] {
        let remote = uri_to_native("file://server/", style).expect_err("no share path");
        assert!(matches!(remote, UriError::NoLocalPath(_)), "{style:?}");
        let scheme = uri_to_native("https://server/a.rs", style).expect_err("not file");
        assert!(
            matches!(scheme, UriError::UnsupportedScheme(_)),
            "{style:?}"
        );
        let encoded = uri_to_native("file:///C:/a%2Fb", style).expect_err("an encoded /");
        assert!(matches!(encoded, UriError::NoLocalPath(_)), "{style:?}");
        let truncated = uri_to_native("file:///C:/a%", style).expect_err("a lone %");
        assert!(
            matches!(truncated, UriError::InvalidUtf8(None)),
            "{style:?}"
        );
    }
}
