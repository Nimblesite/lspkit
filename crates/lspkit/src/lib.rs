//! `lspkit` — generic Rust infrastructure for building LSP+MCP servers.
//!
//! The headline contribution is [`EngineApi`] — the contract that an LSP
//! backend and an MCP server both consume so live analysis state is computed
//! once and vended two ways. See `spec.md` at the workspace root for the full
//! behavioral contract.
//!
//! ```
//! use lspkit::prelude::*;
//! ```

pub mod engine;
pub mod prelude;

#[cfg(feature = "tracing-setup")]
pub mod tracing_setup;

pub use crate::engine::{
    Cause, EngineApi, Generation, GenerationEvent, GenerationEventStream, Progress, RescanScope,
    RescanTicket, Snapshot,
};
