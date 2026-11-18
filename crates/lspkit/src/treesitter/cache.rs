//! Content-hash-keyed parse cache.
//!
//! Parse trees are cached by `(language_name, content_hash)`. Repeated parses
//! of identical source bytes return the cached tree directly. The cache keeps
//! at most [`MAX_TREES`] trees and evicts the oldest first, so editing a
//! document never leaves one tree per version behind. [MEM-060-PARSE-CACHE]

use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, PoisonError};

use dashmap::DashMap;
use tree_sitter::{Language, Parser, Tree};

use crate::treesitter::parser::ParserError;

/// FNV-1a-style content hash. Cheap to compute, sufficient for cache keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash(u64);

impl ContentHash {
    /// Compute a hash of `source`.
    #[must_use]
    pub fn of(source: &[u8]) -> Self {
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        Self(hasher.finish())
    }

    /// Raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Most trees one cache keeps; older trees are evicted first.
pub const MAX_TREES: usize = 128;

/// Cache key: language identity and exact source-content hash.
type Key = (String, ContentHash);

/// Concurrent parse cache.
#[derive(Default, Clone)]
pub struct ParseCache {
    /// Trees shared by language identity and exact source-content hash.
    entries: Arc<DashMap<Key, Arc<Tree>>>,
    /// Keys in insertion order, oldest first; the eviction boundary.
    order: Arc<Mutex<VecDeque<Key>>>,
}

impl ParseCache {
    /// New empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse `source` for `language`, returning a shared tree.
    ///
    /// # Errors
    /// Returns [`ParserError`] if tree-sitter rejects the language or yields no tree.
    pub fn parse(
        &self,
        language_name: &str,
        language: &Language,
        source: &[u8],
    ) -> Result<Arc<Tree>, ParserError> {
        let key = (language_name.to_owned(), ContentHash::of(source));
        if let Some(existing) = self.entries.get(&key) {
            return Ok(Arc::clone(&existing));
        }
        let mut parser = Parser::new();
        parser
            .set_language(language)
            .map_err(|source| ParserError::Language(language_name.to_owned(), source))?;
        self.store(key, parser.parse(source, None))
    }

    /// A native parse without a tree must never populate the shared cache.
    fn store(&self, key: Key, tree: Option<Tree>) -> Result<Arc<Tree>, ParserError> {
        let Some(tree) = tree else {
            return Err(ParserError::NoTree(key.0));
        };
        let arc = Arc::new(tree);
        let mut order = self.order.lock().unwrap_or_else(PoisonError::into_inner);
        // Racing parsers may replace an equivalent tree; either tree remains valid.
        if self.entries.insert(key.clone(), Arc::clone(&arc)).is_none() {
            order.push_back(key);
        }
        while order.len() > MAX_TREES {
            if let Some(oldest) = order.pop_front() {
                let _evicted = self.entries.remove(&oldest);
            }
        }
        Ok(arc)
    }

    /// Drop all cached trees.
    pub fn clear(&self) {
        let mut order = self.order.lock().unwrap_or_else(PoisonError::into_inner);
        order.clear();
        self.entries.clear();
    }

    /// Number of cached trees.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl std::fmt::Debug for ParseCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParseCache")
            .field("entries", &self.entries.len())
            .finish()
    }
}

// The native parser's absent-result path cannot be forced through safe language handles.
#[cfg(test)]
mod tests {
    use super::{ContentHash, ParseCache, ParserError};

    #[test]
    fn a_missing_native_tree_never_enters_the_cache() {
        let cache = ParseCache::new();
        let key = ("rust".to_owned(), ContentHash::of(b"fn main() {}"));
        assert!(
            matches!(cache.store(key.clone(), None), Err(ParserError::NoTree(name)) if name == "rust")
        );
        let message = cache
            .store(key, None)
            .map(|_| ())
            .map_err(|e| e.to_string());
        assert_eq!(
            message,
            Err("tree-sitter produced no tree for language rust".to_owned())
        );
        assert!(cache.is_empty());
    }
}
