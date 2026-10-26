//! Integration tests for the layered TOML config loader.
//!
//! Exercises the public API end-to-end against real temporary directory
//! fixtures. No internals are touched.

use std::fs;
use std::path::PathBuf;

use lspkit_config::{find_ancestor, load_from_ancestor, load_from_path, LoadError, LoadedConfig};
use serde::Deserialize;
use tempfile::TempDir;

/// Minimal config consumer used to drive the generic loader.
#[derive(Debug, Deserialize, PartialEq, Eq)]
struct TestConfig {
    name: String,
    count: u32,
}

const CONFIG_NAME: &str = "lspkit.toml";

/// Build a temp dir holding `lspkit.toml` with the given body, returning both.
fn temp_with_config(body: &str) -> std::io::Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    let path = dir.path().join(CONFIG_NAME);
    fs::write(&path, body)?;
    Ok((dir, path))
}

#[test]
fn find_ancestor_returns_path_when_file_in_start_dir() {
    let (dir, path) = temp_with_config("name = \"x\"\ncount = 1\n").expect("create fixture");
    let found = find_ancestor(dir.path(), CONFIG_NAME).expect("config should be found");
    assert_eq!(found, path);
}

#[test]
fn find_ancestor_walks_up_to_parent_directory() {
    let root = TempDir::new().expect("tempdir");
    let nested = root.path().join("a").join("b").join("c");
    fs::create_dir_all(&nested).expect("create nested");
    let cfg_path = root.path().join(CONFIG_NAME);
    fs::write(&cfg_path, "name = \"deep\"\ncount = 7\n").expect("write");

    let found = find_ancestor(&nested, CONFIG_NAME).expect("should walk up");
    assert_eq!(found, cfg_path);
}

#[test]
fn find_ancestor_returns_none_when_missing() {
    let dir = TempDir::new().expect("tempdir");
    assert!(find_ancestor(dir.path(), "nothing-here.toml").is_none());
}

#[test]
fn find_ancestor_ignores_directories_named_like_target() {
    let dir = TempDir::new().expect("tempdir");
    // Create a *directory* with the config name; loader must skip it.
    fs::create_dir(dir.path().join(CONFIG_NAME)).expect("mkdir");
    assert!(find_ancestor(dir.path(), CONFIG_NAME).is_none());
}

#[test]
fn load_from_path_deserializes_valid_toml() {
    let (_dir, path) = temp_with_config("name = \"alpha\"\ncount = 42\n").expect("create fixture");
    let loaded: LoadedConfig<TestConfig> = load_from_path(&path).expect("load ok");
    assert_eq!(loaded.source, path);
    assert_eq!(
        loaded.value,
        TestConfig {
            name: "alpha".to_owned(),
            count: 42,
        }
    );
}

#[test]
fn load_from_path_returns_io_error_for_missing_file() {
    let dir = TempDir::new().expect("tempdir");
    let missing = dir.path().join("absent.toml");
    let err = load_from_path::<TestConfig>(&missing).expect_err("must error");
    assert!(matches!(&err, LoadError::Io { path, source }
        if path == &missing && source.kind() == std::io::ErrorKind::NotFound));
}

#[test]
fn load_from_path_returns_parse_error_for_invalid_toml() {
    let (_dir, path) = temp_with_config("this is = not valid = toml\n").expect("create fixture");
    let err = load_from_path::<TestConfig>(&path).expect_err("must error");
    assert!(matches!(&err, LoadError::Parse { path: p, message }
        if p == &path && !message.is_empty()));
}

#[test]
fn load_from_path_returns_parse_error_for_wrong_schema() {
    // Valid TOML, wrong shape for `TestConfig` (count is missing).
    let (_dir, path) = temp_with_config("name = \"only\"\n").expect("create fixture");
    let err = load_from_path::<TestConfig>(&path).expect_err("must error");
    assert!(matches!(err, LoadError::Parse { .. }));
}

#[test]
fn load_from_ancestor_finds_and_parses_nested() {
    let root = TempDir::new().expect("tempdir");
    let nested = root.path().join("src").join("module");
    fs::create_dir_all(&nested).expect("create nested");
    let cfg_path = root.path().join(CONFIG_NAME);
    fs::write(&cfg_path, "name = \"root\"\ncount = 3\n").expect("write");

    let loaded: LoadedConfig<TestConfig> =
        load_from_ancestor(&nested, CONFIG_NAME).expect("load ok");
    assert_eq!(loaded.source, cfg_path);
    assert_eq!(loaded.value.name, "root");
    assert_eq!(loaded.value.count, 3);
}

#[test]
fn load_from_ancestor_returns_not_found() {
    let dir = TempDir::new().expect("tempdir");
    let err = load_from_ancestor::<TestConfig>(dir.path(), "no-such-config.toml")
        .expect_err("must error");
    assert!(matches!(&err, LoadError::NotFound(name) if name == "no-such-config.toml"));
}

#[test]
fn load_from_ancestor_surfaces_parse_error() {
    let (dir, _path) = temp_with_config("oops = = =\n").expect("create fixture");
    let err = load_from_ancestor::<TestConfig>(dir.path(), CONFIG_NAME).expect_err("must error");
    assert!(matches!(err, LoadError::Parse { .. }));
}

#[test]
fn load_error_display_messages_are_informative() {
    let not_found = LoadError::NotFound("x.toml".to_owned());
    assert!(format!("{not_found}").contains("x.toml"));

    let parse = LoadError::Parse {
        path: PathBuf::from("/tmp/c.toml"),
        message: "boom".to_owned(),
    };
    let rendered = format!("{parse}");
    assert!(rendered.contains("/tmp/c.toml"));
    assert!(rendered.contains("boom"));
}

#[test]
fn load_error_debug_is_implemented() {
    let err = LoadError::NotFound("dbg.toml".to_owned());
    let dbg = format!("{err:?}");
    assert!(dbg.contains("NotFound"));
}

#[test]
fn loaded_config_debug_is_implemented() {
    let (_dir, path) = temp_with_config("name = \"d\"\ncount = 0\n").expect("create fixture");
    let loaded: LoadedConfig<TestConfig> = load_from_path(&path).expect("load");
    let dbg = format!("{loaded:?}");
    assert!(dbg.contains("LoadedConfig"));
}

#[test]
fn closest_ancestor_wins_over_higher_one() {
    let root = TempDir::new().expect("tempdir");
    let inner = root.path().join("inner");
    fs::create_dir_all(&inner).expect("create inner");

    fs::write(
        root.path().join(CONFIG_NAME),
        "name = \"outer\"\ncount = 1\n",
    )
    .expect("write outer");
    fs::write(inner.join(CONFIG_NAME), "name = \"inner\"\ncount = 2\n").expect("write inner");

    let loaded: LoadedConfig<TestConfig> = load_from_ancestor(&inner, CONFIG_NAME).expect("load");
    assert_eq!(loaded.value.name, "inner");
    assert_eq!(loaded.value.count, 2);
}
