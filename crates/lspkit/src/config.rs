//! `lspkit::config` — layered TOML config loader for LSP/MCP servers.
//!
//! Walks up from a workspace root looking for a named config file and
//! deserializes it into a consumer-supplied `serde::Deserialize` type.

pub mod layered;

pub use crate::config::layered::{
    find_ancestor, load_from_ancestor, load_from_path, LoadError, LoadedConfig,
};
