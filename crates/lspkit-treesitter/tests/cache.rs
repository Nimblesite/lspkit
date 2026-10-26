//! Coarse E2E tests for the parse cache.

use std::sync::Arc;

use lspkit_treesitter::{ContentHash, ParseCache};
use tree_sitter::Language;

fn rust_language() -> Language {
    tree_sitter_rust::LANGUAGE.into()
}

const RUST_SRC: &[u8] = b"fn main() { let x = 42; }";
const RUST_SRC_ALT: &[u8] = b"fn other() { let y = 7; }";

#[test]
fn parse_miss_then_hit_returns_same_arc() {
    let cache = ParseCache::new();
    let language = rust_language();

    let first = cache
        .parse("rust", &language, RUST_SRC)
        .expect("first parse should succeed");
    let second = cache
        .parse("rust", &language, RUST_SRC)
        .expect("second parse should succeed");

    assert!(Arc::ptr_eq(&first, &second), "cache hit must reuse Arc");
    assert_eq!(cache.len(), 1);
    assert!(!cache.is_empty());
}

#[test]
fn parse_different_sources_creates_separate_entries() {
    let cache = ParseCache::new();
    let language = rust_language();

    let tree_a = cache.parse("rust", &language, RUST_SRC).expect("parse a");
    let tree_b = cache
        .parse("rust", &language, RUST_SRC_ALT)
        .expect("parse b");

    assert!(!Arc::ptr_eq(&tree_a, &tree_b));
    assert_eq!(cache.len(), 2);
}

#[test]
fn parse_different_language_names_creates_separate_entries() {
    let cache = ParseCache::new();
    let language = rust_language();

    let _ = cache
        .parse("rust", &language, RUST_SRC)
        .expect("rust parse");
    let _ = cache
        .parse("rust-other", &language, RUST_SRC)
        .expect("aliased parse");

    assert_eq!(cache.len(), 2);
}

#[test]
fn clear_drops_all_entries() {
    let cache = ParseCache::new();
    let language = rust_language();

    let _ = cache
        .parse("rust", &language, RUST_SRC)
        .expect("seed parse");
    assert_eq!(cache.len(), 1);

    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);
}

#[test]
fn default_is_equivalent_to_new() {
    let from_new = ParseCache::new();
    let from_default = ParseCache::default();
    assert!(from_new.is_empty());
    assert!(from_default.is_empty());
    assert_eq!(from_new.len(), from_default.len());
}

#[test]
fn cache_clone_shares_storage() {
    let cache = ParseCache::new();
    let language = rust_language();
    let clone = cache.clone();

    let _ = cache
        .parse("rust", &language, RUST_SRC)
        .expect("primary parse");

    assert_eq!(clone.len(), 1, "Clone must share underlying storage");
    clone.clear();
    assert!(cache.is_empty());
}

#[test]
fn debug_includes_entry_count() {
    let cache = ParseCache::new();
    let language = rust_language();
    let _ = cache
        .parse("rust", &language, RUST_SRC)
        .expect("seed parse");

    let rendered = format!("{cache:?}");
    assert!(rendered.contains("ParseCache"));
    assert!(rendered.contains('1'));
}

#[test]
fn parse_tree_is_well_formed() {
    let cache = ParseCache::new();
    let language = rust_language();

    let tree = cache
        .parse("rust", &language, RUST_SRC)
        .expect("parse should succeed");
    let root = tree.root_node();
    assert_eq!(root.kind(), "source_file");
    assert!(!root.has_error());
    assert!(root.child_count() >= 1);
}

#[test]
fn parse_empty_source_is_cached() {
    let cache = ParseCache::new();
    let language = rust_language();

    let first = cache.parse("rust", &language, b"").expect("empty parse");
    let second = cache.parse("rust", &language, b"").expect("empty re-parse");
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(cache.len(), 1);
}

#[test]
fn content_hash_is_deterministic_and_distinguishes_inputs() {
    let hash_a = ContentHash::of(RUST_SRC);
    let hash_a_again = ContentHash::of(RUST_SRC);
    let hash_b = ContentHash::of(RUST_SRC_ALT);

    assert_eq!(hash_a, hash_a_again);
    assert_ne!(hash_a, hash_b);
    assert_eq!(hash_a.get(), hash_a_again.get());
}

#[test]
fn content_hash_copy_and_debug() {
    let hash = ContentHash::of(b"hello");
    let copied = hash;
    assert_eq!(hash, copied);
    assert_eq!(hash.get(), copied.get());

    let rendered = format!("{hash:?}");
    assert!(rendered.contains("ContentHash"));
}

#[test]
fn content_hash_is_hashable() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    assert!(set.insert(ContentHash::of(b"one")));
    assert!(set.insert(ContentHash::of(b"two")));
    assert!(!set.insert(ContentHash::of(b"one")));
    assert_eq!(set.len(), 2);
}
