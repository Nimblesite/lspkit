//! The analysis core shared by both reference modes: real `serde_json`
//! parses of real files produce real syntax diagnostics ([LSP-PULL-SALSA]),
//! plus the synchronized document sources and the native coalescing watcher
//! that invalidates the snapshot ([LSP-PULL-WATCH]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use lsp_types::{Diagnostic, DiagnosticSeverity, InitializeParams, Position, Range};
use lspkit_live::watcher::CoalescedWatcher;
use lspkit_live::{FileWatcher, ScheduleConfig};
use lspkit_server::pull::{Analysis, AnalysisError};
use lspkit_server::uri_to_path;
use serde_json::Value;

/// One synchronized document snapshot.
#[derive(Clone)]
pub(super) struct Document {
    pub(super) text: String,
    pub(super) version: i32,
}

/// Editor-owned sources: opened documents plus the watched root, and the
/// supporting backend work counter. Clones share one session.
#[derive(Clone, Default)]
pub(super) struct Sources {
    pub(super) documents: Arc<Mutex<HashMap<String, Document>>>,
    pub(super) root: Arc<Mutex<Option<PathBuf>>>,
    analysis_runs: Arc<AtomicUsize>,
}

impl Sources {
    /// Record one real backend run; counts only actual analyzer invocations.
    fn record_run(&self) {
        let _ = self.analysis_runs.fetch_add(1, Ordering::AcqRel);
    }

    /// Backend work performed so far; supporting metadata only.
    pub(super) fn analysis_runs(&self) -> usize {
        self.analysis_runs.load(Ordering::Acquire)
    }
}

/// The compiler callback: real `serde_json` parses of real text.
pub(super) fn analyzer(sources: Sources) -> lspkit_server::pull::Analyzer {
    Arc::new(move |uri| analyze(&sources, uri))
}

/// Analyze one document; read failures stay failures.
fn analyze(sources: &Sources, uri: &str) -> Analysis {
    sources.record_run();
    let text = document_text(sources, uri)?;
    Ok(Arc::new(syntax_diagnostics(&text)))
}

/// Opened document text, or the file content from disk.
fn document_text(sources: &Sources, uri: &str) -> Result<String, AnalysisError> {
    if let Some(document) = sources
        .documents
        .lock()
        .ok()
        .and_then(|documents| documents.get(uri).cloned())
    {
        return Ok(document.text);
    }
    let path = uri_to_path(uri).map_err(|error| AnalysisError(error.to_string()))?;
    std::fs::read_to_string(path)
        .map_err(|error| AnalysisError(format!("document read failed: {error}")))
}

/// The synchronized version of `uri`, or 0 for unopened documents.
pub(super) fn document_version(sources: &Sources, uri: &str) -> i32 {
    sources
        .documents
        .lock()
        .ok()
        .and_then(|documents| documents.get(uri).map(|document| document.version))
        .unwrap_or_default()
}

/// Real syntax diagnostics from a real JSON parse.
pub(super) fn syntax_diagnostics(text: &str) -> Vec<Diagnostic> {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => shape_diagnostics(&value),
        Err(error) => vec![diagnostic(
            error.line(),
            error.column(),
            DiagnosticSeverity::ERROR,
            &error.to_string(),
        )],
    }
}

/// Real AST-shape diagnostics for structurally wrong documents.
fn shape_diagnostics(value: &Value) -> Vec<Diagnostic> {
    if value.is_object() {
        return Vec::new();
    }
    vec![diagnostic(
        1,
        1,
        DiagnosticSeverity::WARNING,
        "top-level JSON value must be an object",
    )]
}

/// One diagnostic at the one-based `line`/`column` source position.
fn diagnostic(
    line: usize,
    column: usize,
    severity: DiagnosticSeverity,
    message: &str,
) -> Diagnostic {
    let position = Position::new(zero_based(line), zero_based(column));
    Diagnostic {
        range: Range::new(position, position),
        severity: Some(severity),
        code: None,
        code_description: None,
        related_information: None,
        tags: None,
        source: Some("serde_json".to_owned()),
        message: message.to_owned(),
        data: None,
    }
}

/// Zero-based LSP coordinate from a one-based parser position.
fn zero_based(one_based: usize) -> u32 {
    u32::try_from(one_based.saturating_sub(1)).unwrap_or_default()
}

/// Watch the workspace root exactly once, feeding the shared scheduler.
pub(super) fn watch_root(sources: &Sources, root: &Path, on_change: impl Fn() + Send + 'static) {
    let unclaimed = match sources.root.lock() {
        Ok(mut watched) if watched.is_none() => watched.replace(root.to_path_buf()).is_none(),
        _ => false,
    };
    if !unclaimed {
        return;
    }
    match FileWatcher::new(root) {
        // `ScheduleConfig` is `#[non_exhaustive]`, so consumers take the
        // shared defaults; the watcher tests exercise real windows anyway.
        Ok(watcher) => {
            drop(tokio::spawn(forward_changes(
                watcher.coalesced(ScheduleConfig::default()),
                on_change,
            )));
        }
        Err(error) => tracing::warn!(error = %error, "reference watcher unavailable"),
    }
}

/// Forward coalesced disk changes as snapshot invalidations.
async fn forward_changes(mut watcher: CoalescedWatcher, on_change: impl Fn()) {
    while watcher.next().await.is_some() {
        on_change();
    }
}

/// The root path from `rootUri` or the first workspace folder.
pub(super) fn root_path(init: &InitializeParams) -> Option<PathBuf> {
    let from_folders = || {
        init.workspace_folders
            .as_ref()?
            .first()
            .map(|folder| folder.uri.as_str().to_owned())
    };
    let uri = from_folders().or_else(|| root_uri_fallback(init))?;
    uri_to_path(&uri).ok()
}

/// The deprecated `rootUri` field, still sent by every current client.
#[allow(deprecated)]
fn root_uri_fallback(init: &InitializeParams) -> Option<String> {
    init.root_uri.as_ref().map(|uri| uri.as_str().to_owned())
}
