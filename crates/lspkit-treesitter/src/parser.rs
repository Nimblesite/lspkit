//! `LanguageParser` trait.
//!
//! A `LanguageParser` knows how to obtain a [`tree_sitter::Language`] and to
//! map a file extension to that language. Concrete grammars are supplied by
//! the consumer; this crate bundles none.

use tree_sitter::Language;

/// A language-specific parser plug-in.
pub trait LanguageParser: Send + Sync + 'static {
    /// Short identifier (e.g. `"rust"`, `"python"`).
    fn name(&self) -> &str;

    /// File extensions (lower-case, without the leading dot) handled by this parser.
    fn extensions(&self) -> &[&str];

    /// The tree-sitter language definition.
    fn language(&self) -> Language;
}

/// Errors emitted by parser registry lookups.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ParserError {
    /// No parser registered for the given extension.
    #[error("no parser registered for extension: {0}")]
    UnknownExtension(String),
    /// Tree-sitter rejected the named language (an incompatible grammar ABI); the
    /// rejection is kept as the source.
    #[error("tree-sitter rejected language {0}")]
    Language(String, #[source] tree_sitter::LanguageError),
}
