//! Content-hash-keyed parse cache.
//!
//! Parse trees are cached by `(language_name, content_hash)`. Repeated parses
//! of identical source bytes return the cached tree directly. Cache eviction
//! is the caller's responsibility — pass `clear()` between sessions.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

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

/// Concurrent parse cache.
#[derive(Default, Clone)]
pub struct ParseCache {
    /// Trees shared by language identity and exact source-content hash.
    entries: Arc<DashMap<(String, ContentHash), Arc<Tree>>>,
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
    /// Returns [`ParserError`] if tree-sitter rejects the language.
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
        let Some(tree) = parser.parse(source, None) else {
            return Err(ParserError::UnknownExtension(language_name.to_owned()));
        };
        let arc = Arc::new(tree);
        // Racing parsers may replace an equivalent tree; either tree remains valid.
        let _ = self.entries.insert(key, Arc::clone(&arc));
        Ok(arc)
    }

    /// Drop all cached trees.
    pub fn clear(&self) {
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
