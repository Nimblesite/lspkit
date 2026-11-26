//! [URI-AUTHORITY] [URI-WINDOWS] Path-style rules, isolated so the Linux CI
//! runs the Windows rules too: the public functions only take the Windows
//! branch on a Windows build, where `tests/path_uri_windows.rs` runs the same table.

use super::{native_to_uri, uri_to_native, Style, UriError};

use super::cases;

#[test]
fn windows_uris_decode_to_exact_paths_and_round_trip() {
    for (uri, path, canonical) in cases::ROUND_TRIPS {
        assert_eq!(
            uri_to_native(uri, Style::Windows).ok().as_deref(),
            Some(path),
            "{uri}"
        );
        assert_eq!(
            native_to_uri(path, Style::Windows).as_deref(),
            Some(canonical),
            "{path}"
        );
        assert_eq!(
            uri_to_native(canonical, Style::Windows).ok().as_deref(),
            Some(path),
            "{canonical} round-trips"
        );
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
        let decoded = uri_to_native(uri, Style::Windows);
        assert!(
            decoded.as_ref().is_ok_and(|native| !native.contains('/')),
            "{uri} decodes with native separators: {decoded:?}"
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
        let rejected = uri_to_native(uri, Style::Windows);
        assert!(
            rejected
                .as_ref()
                .is_err_and(|err| format!("{err:?}").starts_with(variant)),
            "{uri} must be {variant}: {rejected:?}"
        );
        if let Err(UriError::NoLocalPath(named)) = &rejected {
            assert_eq!(named, uri, "the error names the URI");
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
            uri_to_native(uri, Style::Unix).ok().as_deref(),
            Some(path),
            "{uri}"
        );
    }
    assert_eq!(
        native_to_uri("/tmp/a b.rs", Style::Unix).as_deref(),
        Some("file:///tmp/a%20b.rs")
    );
    for style in [Style::Unix, Style::Windows] {
        let remote = uri_to_native("file://server/", style);
        assert!(matches!(remote, Err(UriError::NoLocalPath(_))), "{style:?}");
        let scheme = uri_to_native("https://server/a.rs", style);
        assert!(
            matches!(scheme, Err(UriError::UnsupportedScheme(_))),
            "{style:?}"
        );
        let encoded = uri_to_native("file:///C:/a%2Fb", style);
        assert!(
            matches!(encoded, Err(UriError::NoLocalPath(_))),
            "{style:?}"
        );
        let truncated = uri_to_native("file:///C:/a%", style);
        assert!(
            matches!(truncated, Err(UriError::InvalidUtf8(None))),
            "{style:?}"
        );
    }
}
