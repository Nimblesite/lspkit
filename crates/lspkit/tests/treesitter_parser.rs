//! Coarse E2E tests for `LanguageParser` + `ParserError`.

#![cfg(feature = "treesitter")]

use lspkit::treesitter::{LanguageParser, ParseCache, ParserError};
use tree_sitter::Language;

struct RustParser;

impl LanguageParser for RustParser {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn language(&self) -> Language {
        tree_sitter_rust::LANGUAGE.into()
    }
}

#[test]
fn language_parser_trait_exposes_metadata() {
    let parser: Box<dyn LanguageParser> = Box::new(RustParser);
    assert_eq!(parser.name(), "rust");
    assert_eq!(parser.extensions(), &["rs"]);
}

#[test]
fn language_parser_language_can_drive_cache() {
    let parser = RustParser;
    let cache = ParseCache::new();
    let tree = cache
        .parse(parser.name(), &parser.language(), b"fn x() {}")
        .expect("parse via trait language");
    assert_eq!(tree.root_node().kind(), "source_file");
}

#[test]
fn parser_error_display_contains_extension() {
    let err = ParserError::UnknownExtension("xyz".to_owned());
    let rendered = format!("{err}");
    assert!(rendered.contains("xyz"));
    assert!(rendered.contains("no parser registered"));
}

#[test]
fn parser_error_debug_is_useful() {
    let err = ParserError::UnknownExtension("ml".to_owned());
    let rendered = format!("{err:?}");
    assert!(rendered.contains("UnknownExtension"));
    assert!(rendered.contains("ml"));
}

#[test]
fn parser_error_is_std_error() {
    fn assert_error<T: std::error::Error>(_: &T) {}
    let err = ParserError::UnknownExtension("foo".to_owned());
    assert_error(&err);
}
