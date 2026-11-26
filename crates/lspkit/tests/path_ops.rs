//! [PATH-CENTRAL] Every path operation other crates call, through the public
//! `lspkit::path` API against a real temporary directory.

use std::fs;
use std::path::{Path, PathBuf};

use lspkit::path::{
    canonical, document_uri, find_ancestor, is_within, normalize_path, path_in_roots, path_to_uri,
    rebase, resolve_against, same_path,
};

/// A temporary tree `root/a/b` holding `root/marker.toml` and `root/a/b/leaf.rs`.
fn tree() -> std::io::Result<(tempfile::TempDir, PathBuf)> {
    let dir = tempfile::tempdir()?;
    let root = canonical(dir.path())?;
    fs::create_dir_all(root.join("a").join("b"))?;
    fs::write(root.join("marker.toml"), "x = 1")?;
    fs::write(root.join("a").join("b").join("leaf.rs"), "fn main() {}")?;
    Ok((dir, root))
}

#[test]
fn containment_compares_whole_components() {
    let (_dir, root) = tree().expect("temporary tree");
    let leaf = root.join("a").join("b").join("leaf.rs");
    assert!(is_within(&leaf, &root), "a file under the root");
    assert!(is_within(&root, &root), "the root itself");
    let mut sibling = root.clone().into_os_string();
    sibling.push("2");
    let sibling = PathBuf::from(sibling);
    assert!(
        !is_within(&sibling, &root),
        "a sibling sharing the root's text prefix"
    );
    assert!(
        !is_within(&root, &leaf),
        "a root is not inside its own file"
    );
    assert!(path_in_roots(&leaf, &[sibling.clone(), root.clone()]));
    assert!(!path_in_roots(&sibling, std::slice::from_ref(&root)));
    assert!(!path_in_roots(&leaf, &[]), "no roots contain nothing");
}

#[test]
fn equality_resolution_and_rebasing() {
    let (_dir, root) = tree().expect("temporary tree");
    let leaf = root.join("a").join("b").join("leaf.rs");
    assert!(same_path(&leaf, &root.join("a").join("b").join("leaf.rs")));
    assert!(!same_path(&leaf, &root));
    assert_eq!(
        resolve_against("b/leaf.rs", Some(&root.join("a"))),
        root.join("a").join("b/leaf.rs")
    );
    assert_eq!(
        resolve_against(&leaf, Some(Path::new("ignored"))),
        leaf,
        "absolute is kept"
    );
    assert_eq!(
        resolve_against("b/leaf.rs", None),
        PathBuf::from("b/leaf.rs"),
        "no base"
    );
    let alias = root.join("alias");
    assert_eq!(
        rebase(leaf.clone(), &root, &alias),
        alias.join("a").join("b").join("leaf.rs")
    );
    let outside = PathBuf::from("elsewhere").join("x.rs");
    assert_eq!(
        rebase(outside.clone(), &root, &alias),
        outside,
        "outside the root is kept"
    );
}

#[test]
fn canonicalization_resolves_existing_paths_and_keeps_missing_ones() {
    let (_dir, root) = tree().expect("temporary tree");
    let dotted = root
        .join("a")
        .join("..")
        .join("a")
        .join("b")
        .join("leaf.rs");
    let leaf = root.join("a").join("b").join("leaf.rs");
    assert_eq!(canonical(&dotted).expect("an existing file resolves"), leaf);
    assert_eq!(normalize_path(&dotted), leaf);
    let missing = root.join("missing").join("..").join("gone.rs");
    assert!(
        canonical(&missing).is_err(),
        "a missing path has no canonical form"
    );
    assert_eq!(
        normalize_path(&missing),
        missing,
        "a missing path keeps its spelling"
    );
}

#[test]
fn ancestor_search_walks_up_to_the_nearest_match() {
    let (_dir, root) = tree().expect("temporary tree");
    let deep = root.join("a").join("b");
    assert_eq!(
        find_ancestor(&deep, "marker.toml"),
        Some(root.join("marker.toml"))
    );
    assert_eq!(
        find_ancestor(&deep, "leaf.rs"),
        Some(deep.join("leaf.rs")),
        "start itself first"
    );
    assert_eq!(
        find_ancestor(&root, Path::new("a").join("b").join("leaf.rs")),
        Some(deep.join("leaf.rs")),
        "a relative path below an ancestor"
    );
    assert_eq!(find_ancestor(&deep, "no-such-file.toml"), None);
    assert_eq!(find_ancestor(&deep, "a"), None, "a folder is not a file");
}

#[test]
fn document_uris_are_the_canonical_file_uris() {
    let (_dir, root) = tree().expect("temporary tree");
    let leaf = root.join("a").join("b").join("leaf.rs");
    let uri = path_to_uri(&leaf).expect("an absolute path has a URI");
    assert!(
        uri.starts_with("file://") && uri.ends_with("/a/b/leaf.rs"),
        "{uri}"
    );
    assert_eq!(document_uri(&leaf).expect("document URI").as_str(), uri);
    assert!(
        document_uri(Path::new("relative.rs")).is_err(),
        "a relative path has no URI"
    );
}
