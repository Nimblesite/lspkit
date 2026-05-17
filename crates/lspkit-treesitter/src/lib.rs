//! `lspkit-treesitter` — pluggable tree-sitter parser registry with a
//! content-hash-keyed parse cache. **No grammars are bundled** — consumers
//! depend on whichever grammar crates they need and implement
//! [`parser::LanguageParser`] for each.

pub mod cache;
pub mod parser;

pub use crate::cache::{ContentHash, ParseCache};
pub use crate::parser::{LanguageParser, ParserError};
