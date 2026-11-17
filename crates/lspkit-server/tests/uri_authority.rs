//! [URI-AUTHORITY] Issue #49 through the public API only, so this file also
//! compiles against the pre-fix crate and fails there on the exact symptom.
//! Unix-only: the Windows rows live in `uri_windows.rs`.
#![cfg(not(windows))]

use std::path::Path;

use lspkit_server::{path_to_uri, uri_to_path};

/// A `localhost` or empty authority is the host, never the first path folder.
#[test]
fn localhost_authority_is_the_host_not_a_folder() {
    for uri in [
        "file://localhost/tmp/a.rs",
        "file://LOCALHOST/tmp/a.rs",
        "file:///tmp/a.rs",
    ] {
        let path = uri_to_path(uri);
        assert_eq!(path.as_deref().ok(), Some(Path::new("/tmp/a.rs")), "{uri}");
        assert!(
            path.as_ref().is_ok_and(|path| path.is_absolute()),
            "{uri} never yields a relative path"
        );
        assert_eq!(
            path.as_deref()
                .ok()
                .and_then(|path| path_to_uri(path).ok())
                .as_deref(),
            Some("file:///tmp/a.rs")
        );
    }
    assert!(
        uri_to_path("file://server/share/a.rs").is_err(),
        "a remote host has no Unix path"
    );
}
