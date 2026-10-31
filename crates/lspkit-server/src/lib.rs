//! `lspkit-server` — hand-rolled JSON-RPC LSP server scaffolding.
//!
//! Provides the primitives every Rust LSP server reimplements:
//!
//! - Content-Length framed JSON-RPC 2.0 reader/writer ([`jsonrpc`]).
//! - Method [`dispatch::Dispatcher`] with cancellation-token plumbing.
//! - [`capabilities::Capabilities`] builder that does not leak protocol types.
//! - [`uri`] helpers for `file://` ↔ `PathBuf`.
//! - [`diagnostics::DiagnosticsBus`] for fan-out to LSP and MCP sinks.
//! - [`progress::ProgressSink`] wrapping `window/workDoneProgress`.
//! - Multi-root [`workspace::Workspace`] tracking.
//! - [`live`] file watching, debouncing and analysis scheduling.
//! - `mcp` — an MCP adapter for the same engine (`mcp` feature).
//!
//! All routing happens against an [`lspkit::EngineApi`] + [`lspkit::vfs::Vfs`]
//! pair the consumer supplies. This is the L1 toolkit crate and depends only
//! on `lspkit` ([CRATE-LAYOUT-LAYERS]).

pub mod capabilities;
pub mod diagnostics;
pub mod dispatch;
pub mod jsonrpc;
pub mod live;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod progress;
#[cfg(feature = "pull")]
pub mod pull;
pub mod reply;
pub mod uri;
pub mod workspace;

pub use crate::capabilities::{Capabilities, Feature, TextSync};
pub use crate::diagnostics::{
    Diagnostic, DiagnosticsBatch, DiagnosticsBus, DiagnosticsSink, Severity,
};
pub use crate::dispatch::{
    CancellationHandle, DispatchError, Dispatcher, HandlerResult, RequestInterest,
};
pub use crate::jsonrpc::{FramingError, JsonRpcError, Message, MessageWriter, RequestId};
pub use crate::progress::{NoopReporter, ProgressReporter, ProgressSink, ProgressUpdate};
pub use crate::reply::{EmissionError, ReplyGate};
pub use crate::uri::{path_to_uri, uri_to_path, UriError};
pub use crate::workspace::{Workspace, WorkspaceFolder};
pub use tokio_util::sync::CancellationToken;
