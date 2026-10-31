//! `lspkit` — generic Rust infrastructure for building LSP+MCP servers.
//!
//! The headline contribution is [`EngineApi`] — the contract that an LSP
//! backend and an MCP server both consume so live analysis state is computed
//! once and vended two ways. See `spec.md` at the workspace root for the full
//! behavioral contract.
//!
//! This is the L0 toolkit crate ([CRATE-LAYOUT-LAYERS]). Its modules have no
//! internal dependencies:
//!
//! - [`config`] — layered TOML config loader.
//! - [`vfs`] — concurrent open-document store with position-encoding support.
//! - [`sidecar`] — framed-IPC transport and lifecycle for out-of-process backends.
//! - `treesitter` — parser trait and parse cache (`treesitter` feature).
//!
//! ```
//! use lspkit::prelude::*;
//! ```

pub mod config;
pub mod engine;
pub mod prelude;
pub mod sidecar;
#[cfg(feature = "treesitter")]
pub mod treesitter;
pub mod vfs;

#[cfg(feature = "tracing-setup")]
pub mod tracing_setup;

pub use crate::engine::{
    Cause, EngineApi, Generation, GenerationEvent, GenerationEventStream, Progress, RescanScope,
    RescanTicket, Snapshot,
};
