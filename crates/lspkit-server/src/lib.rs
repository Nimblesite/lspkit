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
//!
//! All routing happens against an [`lspkit::EngineApi`] + [`lspkit_vfs::Vfs`]
//! pair the consumer supplies.

pub mod capabilities;
pub mod diagnostics;
pub mod dispatch;
pub mod jsonrpc;
pub mod progress;
pub mod uri;
pub mod workspace;

pub use crate::capabilities::{Capabilities, Feature, TextSync};
pub use crate::diagnostics::{
    Diagnostic, DiagnosticsBatch, DiagnosticsBus, DiagnosticsSink, Severity,
};
pub use crate::dispatch::{DispatchError, Dispatcher, HandlerResult};
pub use crate::jsonrpc::{FramingError, JsonRpcError, Message, MessageWriter, RequestId};
pub use crate::progress::{NoopReporter, ProgressReporter, ProgressSink, ProgressUpdate};
pub use crate::uri::{path_to_uri, uri_to_path, UriError};
pub use crate::workspace::{Workspace, WorkspaceFolder};
