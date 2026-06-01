//! `lspkit-mcp` — mount an [`lspkit::EngineApi`] implementation as an MCP server.
//!
//! Wraps the official Rust MCP SDK behind a newtype wall so consumers do not
//! depend on the SDK's public API in their own public types. Tools, resources,
//! and prompts are registered through this crate's surface and routed to the
//! engine.

pub mod adapter;
pub mod tools;

pub use crate::adapter::{Adapter, AdapterError, ToolHandler, ToolInvocation};
pub use crate::tools::{ToolDescription, ToolRegistry};
