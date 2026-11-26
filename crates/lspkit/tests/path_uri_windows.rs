//! [URI-WINDOWS] Issue #49's Windows verification plan, through the public
//! API only: `uri_to_path` and `path_to_uri` on a real Windows build, where
//! `cfg(windows)` selects the Windows rules. Only the public functions are
//! used, so this file also compiles against the pre-fix crate.
#![cfg(windows)]

use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::process::Command;

use lspkit::path::{path_to_uri, uri_to_path};

#[path = "../src/path/cases.rs"]
mod cases;

/// A failed check, naming the input that failed and why.
type Checked<T> = Result<T, String>;

/// Fails with `what` unless `holds`.
fn ensure(holds: bool, what: impl FnOnce() -> String) -> Checked<()> {
    if holds {
        Ok(())
    } else {
        Err(what())
    }
}

/// Fails with both values and `what` unless they are equal.
fn same<T: PartialEq + Debug>(left: T, right: T, what: &str) -> Checked<()> {
    ensure(left == right, move || {
        format!("{what}\n  left: {left:?}\n right: {right:?}")
    })
}

/// Names a failed filesystem step.
fn io<T>(step: &str, result: std::io::Result<T>) -> Checked<T> {
    result.map_err(|err| format!("{step}: {err}"))
}

/// `uri_to_path`, naming the URI when it is rejected.
fn decode(uri: &str) -> Checked<PathBuf> {
    uri_to_path(uri).map_err(|err| format!("{uri} decodes: {err:?}"))
}

/// `path_to_uri`, naming the path when it is rejected.
fn encode(path: &Path) -> Checked<String> {
    path_to_uri(path).map_err(|err| format!("{} encodes: {err:?}", path.display()))
}

/// Decodes `uri`, checks the path is absolute and that its canonical URI
/// decodes to the same path again, and returns the path.
fn round_trip(uri: &str, canonical: &str) -> Checked<PathBuf> {
    let path = decode(uri)?;
    ensure(path.is_absolute(), || {
        format!("{uri} decodes to the relative {}", path.display())
    })?;
    same(encode(&path)?.as_str(), canonical, uri)?;
    same(
        decode(canonical)?.as_os_str(),
        path.as_os_str(),
        &format!("{canonical} round-trips"),
    )?;
    Ok(path)
}

#[test]
fn table_uris_decode_to_exact_absolute_paths_and_round_trip() -> Checked<()> {
    for (uri, path, canonical) in cases::ROUND_TRIPS {
        same(
            round_trip(uri, canonical)?.as_os_str(),
            std::ffi::OsStr::new(path),
            uri,
        )?;
    }
    Ok(())
}

#[test]
fn table_paths_encode_to_one_canonical_uri() -> Checked<()> {
    for (path, uri) in cases::PATHS_TO_URIS {
        same(encode(Path::new(path))?.as_str(), uri, path)?;
        let decoded = round_trip(uri, uri)?;
        ensure(!decoded.as_os_str().to_string_lossy().contains('/'), || {
            format!("{uri} decodes with native separators")
        })?;
    }
    Ok(())
}

#[test]
fn table_device_and_rootless_paths_are_errors() {
    for path in cases::PATHS_REJECTED {
        let result = path_to_uri(Path::new(path));
        assert!(result.is_err(), "{path} gave {result:?}");
    }
}

#[test]
fn table_uris_without_an_absolute_path_are_errors_never_relative_paths() {
    for (uri, variant) in cases::URIS_REJECTED {
        let rejected = uri_to_path(uri);
        assert!(
            rejected
                .as_ref()
                .is_err_and(|err| format!("{err:?}").starts_with(variant)),
            "{uri} must be {variant}, got {rejected:?}"
        );
    }
}

/// A fresh directory, and its path as `std::fs::canonicalize` spells it (`\\?\C:\…`).
fn canonical_temp_dir() -> Checked<(tempfile::TempDir, PathBuf)> {
    let dir = io("temp dir", tempfile::tempdir())?;
    let canonical = io("canonicalize", std::fs::canonicalize(dir.path()))?;
    ensure(
        canonical.as_os_str().to_string_lossy().starts_with(r"\\?\"),
        || format!("{} is extended-length", canonical.display()),
    )?;
    Ok((dir, canonical))
}

/// Writes `contents` at the canonical `path`, then proves its URI decodes to
/// a path that opens the same file and round-trips.
fn real_file_round_trips(path: &Path, contents: &str) -> Checked<()> {
    io("write", std::fs::write(path, contents))?;
    let uri = encode(path)?;
    ensure(!uri.contains('?') && !uri.contains("%3F"), || {
        format!("{uri} keeps no verbatim prefix")
    })?;
    let decoded = round_trip(&uri, &uri)?;
    same(
        std::fs::read_to_string(&decoded).ok().as_deref(),
        Some(contents),
        &format!("{uri} → {} opens the file it names", decoded.display()),
    )
}

/// A canonical folder's URI decodes to a path that resolves to the same folder.
fn real_dir_round_trips(dir: &Path) -> Checked<()> {
    let uri = encode(dir)?;
    let decoded = round_trip(&uri, &uri)?;
    same(
        std::fs::canonicalize(&decoded).ok().as_deref(),
        Some(dir),
        &format!("{uri} → {}", decoded.display()),
    )
}

#[test]
fn canonicalized_files_and_folders_round_trip_and_open() -> Checked<()> {
    let (_dir, root) = canonical_temp_dir()?;
    real_file_round_trips(&root.join("a.rs"), "plain")?;
    let spaced = root.join("my repo");
    io("mkdir", std::fs::create_dir(&spaced))?;
    real_file_round_trips(&spaced.join("b c.rs"), "spaced")?;
    real_file_round_trips(
        &root.join("caf\u{e9} \u{65e5}\u{672c} \u{1f980}.rs"),
        "unicode",
    )?;
    real_file_round_trips(&root.join("a #%&+;=@[]{}'`^~ %20.rs"), "reserved")?;
    real_dir_round_trips(&root)?;
    real_dir_round_trips(&spaced)
}

#[test]
fn names_win32_would_rewrite_round_trip_and_open_the_same_file() -> Checked<()> {
    let (_dir, root) = canonical_temp_dir()?;
    for name in [
        "trailing dot.",
        "trailing space ",
        "aux.rs",
        "NUL",
        "CON",
        "com1.txt",
    ] {
        real_file_round_trips(&root.join(name), name)?;
    }
    let folder = root.join("CON");
    io("remove the CON file", std::fs::remove_file(&folder))?;
    io("mkdir CON", std::fs::create_dir(&folder))?;
    real_file_round_trips(&folder.join("a.rs"), "inside CON")?;
    ensure(root.join("NUL").is_file(), || {
        "NUL stayed a file, never the device".to_owned()
    })
}

#[test]
fn a_path_longer_than_max_path_round_trips_and_opens() -> Checked<()> {
    let (_dir, root) = canonical_temp_dir()?;
    let mut deep = root;
    while deep.as_os_str().len() < 300 {
        deep.push("a-forty-character-folder-name-0123456789");
    }
    io(
        "mkdir under the verbatim root",
        std::fs::create_dir_all(&deep),
    )?;
    let file = deep.join("long.rs");
    real_file_round_trips(&file, "long")?;
    let uri = encode(&file)?;
    ensure(uri.len() > 260, || format!("{uri} is past MAX_PATH"))
}

#[test]
fn a_junction_round_trips_and_opens_through_its_target() -> Checked<()> {
    let (_dir, root) = canonical_temp_dir()?;
    let target = root.join("target");
    io("mkdir", std::fs::create_dir(&target))?;
    io("write", std::fs::write(target.join("a.rs"), "via junction"))?;
    let plain = |path: &Path| {
        path.as_os_str()
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_owned()
    };
    let link = root.join("link");
    let mklink = Command::new("cmd")
        .args(["/C", "mklink", "/J", &plain(&link), &plain(&target)])
        .output();
    let mklink = io("mklink runs", mklink)?;
    ensure(mklink.status.success(), || format!("mklink /J: {mklink:?}"))?;
    let through = link.join("a.rs");
    let uri = encode(&through)?;
    let decoded = round_trip(&uri, &uri)?;
    same(
        std::fs::read_to_string(&decoded).ok().as_deref(),
        Some("via junction"),
        &uri,
    )?;
    let resolved = io("canonicalize", std::fs::canonicalize(&through))?;
    real_file_round_trips(&resolved, "via junction")
}

#[test]
fn drive_letter_case_never_gives_one_file_two_uris() -> Checked<()> {
    let (_dir, root) = canonical_temp_dir()?;
    let file = root.join("Case.rs");
    io("write", std::fs::write(&file, "x"))?;
    let upper = encode(&file)?;
    let text = file.as_os_str().to_string_lossy().into_owned();
    let mut chars = text.trim_start_matches(r"\\?\").chars();
    let drive = chars.next().ok_or("a drive letter")?.to_ascii_lowercase();
    let lower = format!("{drive}{}", chars.as_str());
    ensure(drive.is_ascii_lowercase(), || {
        format!("{lower} starts with a lower-case drive")
    })?;
    same(encode(Path::new(&lower))?.as_str(), upper.as_str(), &lower)
}

#[test]
fn an_8_3_short_name_round_trips_and_opens() -> Checked<()> {
    let uri = encode(Path::new(r"C:\PROGRA~1"))?;
    same(uri.as_str(), "file:///C:/PROGRA~1", "8.3 name")?;
    let decoded = round_trip(&uri, &uri)?;
    let long = io(
        "Program Files exists",
        std::fs::canonicalize(r"C:\Program Files"),
    )?;
    same(
        std::fs::canonicalize(&decoded).ok().as_deref(),
        Some(long.as_path()),
        "C:\\PROGRA~1 opens Program Files (8.3 names are on for the system drive)",
    )?;
    real_dir_round_trips(&long)
}
