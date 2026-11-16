//! [URI-AUTHORITY] Windows path rules, isolated so the Linux CI runs them too:
//! the public functions only take the Windows branch on a Windows build.

use super::{native_to_uri, uri_to_native, Style, UriError};

/// URIs, the Windows path each names, and the URI that path encodes back to.
const WINDOWS_ROUND_TRIPS: [(&str, &str, &str); 5] = [
    (
        "file:///C:/repo/a.rs",
        r"C:\repo\a.rs",
        "file:///C:/repo/a.rs",
    ),
    (
        "file:///c%3A/my%20repo/a.rs",
        r"c:\my repo\a.rs",
        "file:///c:/my%20repo/a.rs",
    ),
    ("file://localhost/D:/a.rs", r"D:\a.rs", "file:///D:/a.rs"),
    (
        "file://server/share/a.rs",
        r"\\server\share\a.rs",
        "file://server/share/a.rs",
    ),
    (
        "file://wsl.localhost/Ubuntu/home/a.rs",
        r"\\wsl.localhost\Ubuntu\home\a.rs",
        "file://wsl.localhost/Ubuntu/home/a.rs",
    ),
];

/// Extended-length paths `std::fs::canonicalize` returns, and their URIs.
const VERBATIM_PATHS: [(&str, &str); 2] = [
    (r"\\?\C:\repo\a.rs", "file:///C:/repo/a.rs"),
    (r"\\?\UNC\server\share\a.rs", "file://server/share/a.rs"),
];

/// URIs that name no absolute Windows path.
const NO_WINDOWS_PATH: [&str; 4] = [
    "file:///tmp/a.rs",
    "file:///C:",
    "file://server",
    "file://server/",
];

#[test]
fn windows_uris_decode_to_absolute_drive_and_unc_paths_and_round_trip() {
    for (uri, path, encoded) in WINDOWS_ROUND_TRIPS {
        let decoded = uri_to_native(uri, Style::Windows).expect("a Windows URI decodes");
        assert_eq!(decoded, path, "{uri}");
        assert_eq!(native_to_uri(&decoded, Style::Windows), encoded, "{path}");
        let again = uri_to_native(encoded, Style::Windows).expect("re-decodes");
        assert_eq!(again, path, "{encoded} round-trips");
    }
}

#[test]
fn windows_extended_length_prefixes_are_removed_from_uris() {
    for (path, uri) in VERBATIM_PATHS {
        assert_eq!(native_to_uri(path, Style::Windows), uri, "{path}");
        let decoded = uri_to_native(uri, Style::Windows).expect("decodes");
        assert!(!decoded.contains('?'), "{uri} decodes without a prefix");
    }
}

#[test]
fn windows_uris_without_an_absolute_path_are_rejected() {
    for uri in NO_WINDOWS_PATH {
        let err = uri_to_native(uri, Style::Windows).expect_err("no absolute path");
        assert!(
            matches!(&err, UriError::NoLocalPath(rejected) if rejected == uri),
            "{uri}: {err:?}"
        );
    }
}
