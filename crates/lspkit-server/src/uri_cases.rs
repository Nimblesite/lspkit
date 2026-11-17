//! [URI-WINDOWS] The Windows conversion table from issue #49, kept once and
//! run twice: by `uri_style_tests.rs` against the Windows rules on every OS,
//! and by `tests/uri_windows.rs` through the public API on a Windows build.
//! Test data only; nothing here ships in the library.

/// A URI, the exact Windows path it decodes to, and the canonical URI that
/// path encodes back to (which decodes to the same path again).
pub(crate) const ROUND_TRIPS: [(&str, &str, &str); 35] = [
    // Drive letters: one URI per drive, the letter upper case.
    (
        "file:///C:/repo/a.rs",
        r"C:\repo\a.rs",
        "file:///C:/repo/a.rs",
    ),
    (
        "file:///c:/repo/a.rs",
        r"C:\repo\a.rs",
        "file:///C:/repo/a.rs",
    ),
    (
        "file:///C%3A/repo/a.rs",
        r"C:\repo\a.rs",
        "file:///C:/repo/a.rs",
    ),
    (
        "file:///c%3a/my%20repo/a.rs",
        r"C:\my repo\a.rs",
        "file:///C:/my%20repo/a.rs",
    ),
    ("file:///C:/", r"C:\", "file:///C:/"),
    (
        "file:///D:/deep/x/y/z/a.rs",
        r"D:\deep\x\y\z\a.rs",
        "file:///D:/deep/x/y/z/a.rs",
    ),
    (
        "file://localhost/C:/repo/a.rs",
        r"C:\repo\a.rs",
        "file:///C:/repo/a.rs",
    ),
    (
        "file://LOCALHOST/C:/repo/a.rs",
        r"C:\repo\a.rs",
        "file:///C:/repo/a.rs",
    ),
    ("file:///C:/repo/", r"C:\repo\", "file:///C:/repo/"),
    (
        "file:///C:/repo/./sub/../a.rs",
        r"C:\repo\a.rs",
        "file:///C:/repo/a.rs",
    ),
    (
        "file:///C:/PROGRA~1/a.rs",
        r"C:\PROGRA~1\a.rs",
        "file:///C:/PROGRA~1/a.rs",
    ),
    // UNC and network: the host is the server, lower case.
    (
        "file://server/share/a.rs",
        r"\\server\share\a.rs",
        "file://server/share/a.rs",
    ),
    (
        "file://server/share/",
        r"\\server\share\",
        "file://server/share/",
    ),
    (
        "file://server/share",
        r"\\server\share",
        "file://server/share",
    ),
    (
        "file:////server/share/a.rs",
        r"\\server\share\a.rs",
        "file://server/share/a.rs",
    ),
    (
        "file://SERVER/Share/a.rs",
        r"\\server\Share\a.rs",
        "file://server/Share/a.rs",
    ),
    (
        "file://wsl.localhost/Ubuntu/home/a.rs",
        r"\\wsl.localhost\Ubuntu\home\a.rs",
        "file://wsl.localhost/Ubuntu/home/a.rs",
    ),
    (
        "file://wsl$/Ubuntu/home/a.rs",
        r"\\wsl$\Ubuntu\home\a.rs",
        "file://wsl%24/Ubuntu/home/a.rs",
    ),
    (
        "file://192.168.1.5/share/a.rs",
        r"\\192.168.1.5\share\a.rs",
        "file://192.168.1.5/share/a.rs",
    ),
    (
        "file://fe80--1.ipv6-literal.net/share/a.rs",
        r"\\fe80--1.ipv6-literal.net\share\a.rs",
        "file://fe80--1.ipv6-literal.net/share/a.rs",
    ),
    (
        "file://[FE80::1%25eth0]/share/a.rs",
        r"\\fe80--1seth0.ipv6-literal.net\share\a.rs",
        "file://fe80--1seth0.ipv6-literal.net/share/a.rs",
    ),
    (
        "file://build%2Dhost/share%20one/a.rs",
        r"\\build-host\share one\a.rs",
        "file://build-host/share%20one/a.rs",
    ),
    // Names Win32 would rewrite keep the `\\?\` form, so they open the same file.
    (
        "file:///C:/repo/name./a.rs",
        r"\\?\C:\repo\name.\a.rs",
        "file:///C:/repo/name./a.rs",
    ),
    (
        "file:///C:/repo/name%20/a.rs",
        r"\\?\C:\repo\name \a.rs",
        "file:///C:/repo/name%20/a.rs",
    ),
    (
        "file:///C:/repo/NUL",
        r"\\?\C:\repo\NUL",
        "file:///C:/repo/NUL",
    ),
    (
        "file:///C:/repo/CON/a.rs",
        r"\\?\C:\repo\CON\a.rs",
        "file:///C:/repo/CON/a.rs",
    ),
    (
        "file:///C:/repo/aux.rs",
        r"\\?\C:\repo\aux.rs",
        "file:///C:/repo/aux.rs",
    ),
    (
        "file://server/share/name./a.rs",
        r"\\?\UNC\server\share\name.\a.rs",
        "file://server/share/name./a.rs",
    ),
    // Characters that must be percent-encoded, and a literal `%20` in a name.
    (
        "file:///C:/a%20%23%25%3F%26%2B%3B%3D%40%5B%5D%7B%7D%27%60%5E~/b.rs",
        r"C:\a #%?&+;=@[]{}'`^~\b.rs",
        "file:///C:/a%20%23%25%3F%26%2B%3B%3D%40%5B%5D%7B%7D%27%60%5E~/b.rs",
    ),
    (
        "file:///C:/a%2520b.rs",
        r"C:\a%20b.rs",
        "file:///C:/a%2520b.rs",
    ),
    (
        "file:///C:/a%252Fb%255Cc.rs",
        r"C:\a%2Fb%5Cc.rs",
        "file:///C:/a%252Fb%255Cc.rs",
    ),
    // Unicode: precomposed, CJK, astral, combining mark, right-to-left.
    (
        "file:///C:/caf%C3%A9.rs",
        "C:\\caf\u{e9}.rs",
        "file:///C:/caf%C3%A9.rs",
    ),
    (
        "file:///C:/%E6%97%A5%E6%9C%AC%E8%AA%9E/%F0%9F%A6%80.rs",
        "C:\\\u{65e5}\u{672c}\u{8a9e}\\\u{1f980}.rs",
        "file:///C:/%E6%97%A5%E6%9C%AC%E8%AA%9E/%F0%9F%A6%80.rs",
    ),
    (
        "file:///C:/e%CC%81.rs",
        "C:\\e\u{301}.rs",
        "file:///C:/e%CC%81.rs",
    ),
    (
        "file:///C:/%D7%A9%D7%9C%D7%95%D7%9D.rs",
        "C:\\\u{5e9}\u{5dc}\u{5d5}\u{5dd}.rs",
        "file:///C:/%D7%A9%D7%9C%D7%95%D7%9D.rs",
    ),
];

/// Windows paths a URI is made from, and the one canonical URI each gets.
pub(crate) const PATHS_TO_URIS: [(&str, &str); 14] = [
    (r"C:/repo\sub/a.rs", "file:///C:/repo/sub/a.rs"),
    (r"C:\repo\\double\a.rs", "file:///C:/repo/double/a.rs"),
    (r"C:\Repo\a.rs", "file:///C:/Repo/a.rs"),
    (r"c:\Repo\a.rs", "file:///C:/Repo/a.rs"),
    (r"\\?\C:\repo\a.rs", "file:///C:/repo/a.rs"),
    (r"\\?\c:\repo\a.rs", "file:///C:/repo/a.rs"),
    (r"\\?\UNC\server\share\a.rs", "file://server/share/a.rs"),
    (r"\\?\C:\repo\name.", "file:///C:/repo/name."),
    (r"\\server\share\a.rs", "file://server/share/a.rs"),
    (r"\\SERVER\share\a.rs", "file://server/share/a.rs"),
    (r"\\server\share\", "file://server/share/"),
    (r"//server/share/a.rs", "file://server/share/a.rs"),
    (r"\\wsl$\Ubuntu\home\a.rs", "file://wsl%24/Ubuntu/home/a.rs"),
    (
        r"\\wsl.localhost\Ubuntu\home\a.rs",
        "file://wsl.localhost/Ubuntu/home/a.rs",
    ),
];

/// Windows paths that name no file a URI can reach: device namespaces,
/// volume GUIDs, a server with no share, and paths without a drive root.
pub(crate) const PATHS_REJECTED: [&str; 9] = [
    r"\\.\C:\a.rs",
    r"\\?\Volume{00000000-0000-0000-0000-000000000000}\",
    r"\\?\GLOBALROOT\Device\x",
    r"\\server\",
    r"\\.\pipe\name",
    r"C:",
    r"C:a.rs",
    r"\a.rs",
    r"repo\a.rs",
];

/// URIs that name no absolute Windows path, with the `UriError` variant each
/// is rejected with. None may decode to a relative path or panic.
pub(crate) const URIS_REJECTED: [(&str, &str); 23] = [
    ("file:repo/a.rs", "UnsupportedScheme"),
    ("http://x/a.rs", "UnsupportedScheme"),
    ("", "UnsupportedScheme"),
    ("file://", "NoLocalPath"),
    ("file:///", "NoLocalPath"),
    ("file:///repo/a.rs", "NoLocalPath"),
    ("file:///tmp/a.rs", "NoLocalPath"),
    ("file:///C:", "NoLocalPath"),
    ("file://server", "NoLocalPath"),
    ("file://server/", "NoLocalPath"),
    ("file://server:8080/share/a.rs", "NoLocalPath"),
    ("file://user@server/share/a.rs", "NoLocalPath"),
    ("file://./C:/a.rs", "NoLocalPath"),
    ("file://%3F/C:/a.rs", "NoLocalPath"),
    ("file:///C:/a%2Fb.rs", "NoLocalPath"),
    ("file:///C:/a%5Cb.rs", "NoLocalPath"),
    ("file://host%5Cshare%5Cx/a.rs", "NoLocalPath"),
    ("file://host%2Fshare/a.rs", "NoLocalPath"),
    ("file:///C:/repo/a%2F..%2F..%2Fwindows.rs", "NoLocalPath"),
    ("file:///C:/a%ZZ.rs", "InvalidUtf8"),
    ("file:///C:/a%", "InvalidUtf8"),
    ("file:///C:/a%2", "InvalidUtf8"),
    ("file:///C:/%FF%FE.rs", "InvalidUtf8"),
];
