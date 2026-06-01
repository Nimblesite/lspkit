//! Diagnostics fan-out bus.
//!
//! A single source of diagnostics (the engine) fans out to multiple sinks
//! atomically per generation. The LSP sink calls `publishDiagnostics`; the
//! MCP sink emits resource-update notifications.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use lspkit::Generation;
use lspkit_vfs::DocumentUri;

use crate::uri::path_to_uri;

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

impl std::fmt::Debug for DiagnosticsBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.sinks.lock().map_or(0, |g| g.len());
        f.debug_struct("DiagnosticsBus")
            .field("sinks", &count)
            .finish()
    }
}

/// Convenience: build a `DocumentUri` from an absolute path.
///
/// # Errors
/// Returns [`crate::uri::UriError`] if the path cannot be expressed as a URI.
pub fn document_uri_from_path(path: &std::path::Path) -> Result<DocumentUri, crate::uri::UriError> {
    Ok(DocumentUri::new(path_to_uri(path)?))
}
