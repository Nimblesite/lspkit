//! End-to-end tests for [`lspkit_server::workspace`].

use std::path::{Path, PathBuf};

use lspkit_server::{Workspace, WorkspaceFolder};

fn folder(root: &str, name: &str) -> WorkspaceFolder {
    WorkspaceFolder::new(PathBuf::from(root), name.to_owned())
}

#[test]
fn new_workspace_is_empty() {
    let ws = Workspace::new();
    assert!(ws.is_empty());
    assert_eq!(ws.len(), 0);
    assert!(ws.folders().is_empty());
}

#[test]
fn default_workspace_matches_new() {
    let lhs = Workspace::default();
    let rhs = Workspace::new();
    assert_eq!(lhs.len(), rhs.len());
}

#[test]
fn add_appends_folder() {
    let ws = Workspace::new();
    ws.add(folder("/a", "alpha"));
    ws.add(folder("/b", "beta"));
    assert_eq!(ws.len(), 2);
    assert!(!ws.is_empty());
    let folders = ws.folders();
    assert_eq!(folders[0].name, "alpha");
    assert_eq!(folders[1].name, "beta");
}

#[test]
fn set_folders_replaces_existing() {
    let ws = Workspace::new();
    ws.add(folder("/old", "old"));
    ws.set_folders(vec![folder("/new1", "n1"), folder("/new2", "n2")]);
    assert_eq!(ws.len(), 2);
    let folders = ws.folders();
    assert_eq!(folders[0].root, Path::new("/new1"));
    assert_eq!(folders[1].root, Path::new("/new2"));
}

#[test]
fn remove_drops_matching_folder_only() {
    let ws = Workspace::new();
    ws.set_folders(vec![
        folder("/a", "alpha"),
        folder("/b", "beta"),
        folder("/c", "gamma"),
    ]);
    ws.remove(Path::new("/b"));
    assert_eq!(ws.len(), 2);
    let names: Vec<String> = ws.folders().into_iter().map(|f| f.name).collect();
    assert!(names.contains(&"alpha".to_owned()));
    assert!(names.contains(&"gamma".to_owned()));
    assert!(!names.contains(&"beta".to_owned()));
}

#[test]
fn remove_unknown_path_is_noop() {
    let ws = Workspace::new();
    ws.set_folders(vec![folder("/a", "alpha")]);
    ws.remove(Path::new("/does/not/exist"));
    assert_eq!(ws.len(), 1);
}

#[test]
fn folder_for_returns_containing_root() {
    let ws = Workspace::new();
    ws.set_folders(vec![folder("/proj/a", "a"), folder("/proj/b", "b")]);
    let found = ws
        .folder_for(Path::new("/proj/a/src/lib.rs"))
        .expect("found");
    assert_eq!(found.name, "a");
}

#[test]
fn folder_for_returns_none_when_no_match() {
    let ws = Workspace::new();
    ws.set_folders(vec![folder("/proj/a", "a")]);
    assert!(ws.folder_for(Path::new("/elsewhere/foo.rs")).is_none());
}

#[test]
fn clone_shares_state() {
    let ws = Workspace::new();
    ws.add(folder("/a", "a"));
    let clone = ws.clone();
    clone.add(folder("/b", "b"));
    assert_eq!(ws.len(), 2);
    assert_eq!(clone.len(), 2);
}

#[test]
fn workspace_folder_eq_and_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(folder("/a", "x"));
    set.insert(folder("/a", "x"));
    set.insert(folder("/a", "y"));
    assert_eq!(set.len(), 2);
}

#[test]
fn workspace_folder_debug_works() {
    let f = folder("/a", "alpha");
    let rendered = format!("{f:?}");
    assert!(rendered.contains("WorkspaceFolder"));
    assert!(rendered.contains("alpha"));
}

#[test]
fn workspace_debug_works() {
    let ws = Workspace::new();
    let rendered = format!("{ws:?}");
    assert!(rendered.contains("Workspace"));
}
