//! Diagnostics fan-out bus.
//!
//! A single source of diagnostics (the engine) fans out to multiple sinks
//! atomically per generation. The LSP sink calls `publishDiagnostics`; the
//! MCP sink emits resource-update notifications.

use std::sync::{Arc, Mutex, Weak};

use async_trait::async_trait;
use lspkit::vfs::DocumentUri;
use lspkit::Generation;

/// The path → [`DocumentUri`] conversion, kept in `lspkit::path` with every
/// other path operation ([PATH-CENTRAL]).
pub use lspkit::path::document_uri as document_uri_from_path;

/// Severity ladder matching the LSP convention.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Error.
    Error,
    /// Warning.
    Warning,
    /// Informational.
    Information,
    /// Hint.
    Hint,
}

/// A single diagnostic finding.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Severity ladder.
    pub severity: Severity,
    /// Human-readable message.
    pub message: String,
    /// Zero-based `(start_line, start_char, end_line, end_char)` range.
    pub range: (u32, u32, u32, u32),
    /// Optional diagnostic code (e.g. lint rule name).
    pub code: Option<String>,
    /// Optional source identifier (e.g. linter name).
    pub source: Option<String>,
}

impl Diagnostic {
    /// Construct a diagnostic with required fields. Optional fields default to `None`.
    #[must_use]
    pub fn new(
        severity: Severity,
        message: impl Into<String>,
        range: (u32, u32, u32, u32),
    ) -> Self {
        Self {
            severity,
            message: message.into(),
            range,
            code: None,
            source: None,
        }
    }

    /// Attach a diagnostic `code`.
    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Attach a `source` identifier.
    #[must_use]
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

/// Diagnostics for a single document at a specific engine generation.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct DiagnosticsBatch {
    /// The document the diagnostics apply to.
    pub uri: DocumentUri,
    /// The engine generation at which they were computed.
    pub generation: Generation,
    /// The findings.
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticsBatch {
    /// Construct a batch tagging `diagnostics` for `uri` at `generation`.
    #[must_use]
    pub fn new(uri: DocumentUri, generation: Generation, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            uri,
            generation,
            diagnostics,
        }
    }
}

/// A consumer of diagnostic batches.
#[async_trait]
pub trait DiagnosticsSink: Send + Sync + 'static {
    /// Receive a batch of diagnostics. Implementations MUST NOT block.
    async fn publish(&self, batch: DiagnosticsBatch);
}

/// Fan-out bus connecting one diagnostics source to many sinks.
#[derive(Clone, Default)]
pub struct DiagnosticsBus {
    /// Shared sink registry snapshotted before asynchronous fan-out begins.
    sinks: Arc<Mutex<Vec<Arc<dyn DiagnosticsSink>>>>,
}

impl DiagnosticsBus {
    /// New bus with no sinks attached.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a sink. Returns the new sink count.
    pub fn attach(&self, sink: Arc<dyn DiagnosticsSink>) -> usize {
        if let Ok(mut guard) = self.sinks.lock() {
            guard.push(sink);
            guard.len()
        } else {
            0
        }
    }

    /// Attach a sink for as long as the returned guard lives: dropping it
    /// detaches the sink, so a per-connection consumer never accumulates on
    /// a long-lived bus. [MEM-LIFETIME-DIAGNOSTICS-SINK]
    pub fn attach_owned(&self, sink: Arc<dyn DiagnosticsSink>) -> SinkRegistration {
        let registered = Arc::downgrade(&sink);
        let _count = self.attach(sink);
        SinkRegistration {
            sinks: Arc::downgrade(&self.sinks),
            sink: registered,
        }
    }

    /// Publish a batch to every attached sink concurrently.
    pub async fn publish(&self, batch: DiagnosticsBatch) {
        let snapshot: Vec<Arc<dyn DiagnosticsSink>> = self
            .sinks
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        for sink in snapshot {
            let sink_batch = batch.clone();
            sink.publish(sink_batch).await;
        }
    }
}

/// Keeps a sink attached to a [`DiagnosticsBus`]; dropping it detaches the sink.
#[must_use = "dropping the registration detaches the sink"]
pub struct SinkRegistration {
    /// The bus's registry; weak, so the guard never keeps the bus alive.
    sinks: Weak<Mutex<Vec<Arc<dyn DiagnosticsSink>>>>,
    /// The attached sink, matched by address on detach.
    sink: Weak<dyn DiagnosticsSink>,
}

impl Drop for SinkRegistration {
    fn drop(&mut self) {
        let Some(sinks) = self.sinks.upgrade() else {
            return;
        };
        let Ok(mut guard) = sinks.lock() else {
            return;
        };
        guard.retain(|sink| !std::ptr::addr_eq(Arc::as_ptr(sink), self.sink.as_ptr()));
    }
}

impl std::fmt::Debug for SinkRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SinkRegistration").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for DiagnosticsBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.sinks.lock().map_or(0, |g| g.len());
        f.debug_struct("DiagnosticsBus")
            .field("sinks", &count)
            .finish()
    }
}
