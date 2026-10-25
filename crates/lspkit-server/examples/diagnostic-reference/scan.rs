//! The workspace scan: every known document under one snapshot, streamed as
//! `$/progress` partials before the gated terminal reply, and restarted when
//! the snapshot revision moves mid-scan ([LSP-PULL-CONCURRENCY]).

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::task::yield_now;

use lspkit_server::pull::PullError;
use lspkit_server::{CancellationToken, HandlerResult, JsonRpcError, ReplyGate};

use crate::analysis;
use crate::session::{document_report, Server};

/// Scan restart budget when the snapshot revision moves mid-scan.
pub(super) const MAX_SCAN_PASSES: usize = 3;

/// `workspace/diagnostic` request parameters.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceParams {
    previous_result_ids: Vec<PreviousResult>,
    partial_result_token: Option<Value>,
}

/// One client-known snapshot identity.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviousResult {
    uri: String,
    value: String,
}

/// Terminal outcome of one scan attempt.
enum ScanOutcome {
    Complete(HandlerResult),
    Interrupted,
}

/// Supersession retries the scan; real compiler and protocol errors stay errors.
enum ScanError {
    Superseded,
    Request(JsonRpcError),
}

/// Scan every known document, retrying while the snapshot keeps moving.
pub(super) async fn run_workspace_scan(
    server: &Server,
    gate: &ReplyGate,
    raw: Value,
    cancel: &CancellationToken,
) -> HandlerResult {
    let Ok(params) = serde_json::from_value::<WorkspaceParams>(raw) else {
        return HandlerResult::Err(JsonRpcError::new(
            -32602,
            "malformed workspace/diagnostic params",
        ));
    };
    for _ in 0..MAX_SCAN_PASSES {
        match scan_once(server, &params, cancel, gate).await {
            Ok(ScanOutcome::Complete(result)) => return result,
            Err(error) => return HandlerResult::Err(error),
            Ok(ScanOutcome::Interrupted) => {
                tracing::warn!("workspace snapshot superseded mid-scan; retrying");
            }
        }
    }
    HandlerResult::Err(JsonRpcError::new(
        -32803,
        "workspace snapshot kept changing during scan",
    ))
}

/// One pass over every scanned document under a single snapshot.
///
/// The cooperative yield between documents is the scheduling handoff that
/// lets the reader trip cancellation or invalidation mid-scan.
async fn scan_once(
    server: &Server,
    params: &WorkspaceParams,
    cancel: &CancellationToken,
    gate: &ReplyGate,
) -> Result<ScanOutcome, JsonRpcError> {
    let revision = server.pull.revision();
    let mut items = Vec::new();
    for uri in scanned_documents(&server.sources)? {
        if cancel.is_cancelled() {
            return Err(cancelled_error());
        }
        if server.pull.revision() != revision {
            return Ok(ScanOutcome::Interrupted);
        }
        match scan_in_background(server, params, gate, &uri, &mut items, cancel).await {
            Ok(()) => {}
            Err(ScanError::Superseded) => return Ok(ScanOutcome::Interrupted),
            Err(ScanError::Request(error)) => return Err(error),
        }
        yield_now().await;
    }
    Ok(finish_scan(server, revision, &items))
}

/// The completed pass remains valid only under the revision that produced it.
fn finish_scan(server: &Server, revision: u64, items: &[Value]) -> ScanOutcome {
    if server.pull.revision() == revision {
        ScanOutcome::Complete(HandlerResult::Ok(json!({ "items": items })))
    } else {
        ScanOutcome::Interrupted
    }
}

/// Cancellation and editor input stay on the foreground while actual I/O runs.
async fn scan_in_background(
    server: &Server,
    params: &WorkspaceParams,
    gate: &ReplyGate,
    uri: &str,
    items: &mut Vec<Value>,
    cancel: &CancellationToken,
) -> Result<(), ScanError> {
    let computation = start_document(server, params, gate, uri);
    let result = lspkit_sidecar::cancellation::until_cancelled(computation, Some(cancel))
        .await
        .map_err(|_| ScanError::Request(cancelled_error()))?;
    items.extend(
        result
            .map_err(|error| ScanError::Request(JsonRpcError::new(-32603, error.to_string())))??,
    );
    Ok(())
}

/// The worker uses the original reply gate even if cancellation finishes first.
fn start_document(
    server: &Server,
    params: &WorkspaceParams,
    gate: &ReplyGate,
    uri: &str,
) -> tokio::task::JoinHandle<Result<Vec<Value>, ScanError>> {
    let server = server.clone();
    let params = params.clone();
    let gate = gate.clone();
    let uri = uri.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut items = Vec::new();
        scan_document(&server, &params, &gate, &uri, &mut items)?;
        Ok(items)
    })
}

/// Analyze one document, streaming it as a partial when a token is set.
fn scan_document(
    server: &Server,
    params: &WorkspaceParams,
    gate: &ReplyGate,
    uri: &str,
    items: &mut Vec<Value>,
) -> Result<(), ScanError> {
    let version = analysis::document_version(&server.sources, uri);
    let previous = previous_identity(params, uri);
    let report = server.pull.diagnostics(uri, version).map_err(scan_error)?;
    let mut item = document_report(&report, previous).map_err(ScanError::Request)?;
    stamp_document(&mut item, uri, version).map_err(ScanError::Request)?;
    if let Some(token) = &params.partial_result_token {
        emit_partial(server, gate, token, &item).map_err(ScanError::Request)
    } else {
        items.push(item);
        Ok(())
    }
}

/// Classify a stale snapshot by its type, never by matching error messages.
fn scan_error(error: PullError) -> ScanError {
    match error {
        PullError::Superseded => ScanError::Superseded,
        PullError::Cancelled => ScanError::Request(cancelled_error()),
        error @ PullError::Analysis(_) => {
            ScanError::Request(JsonRpcError::new(-32603, error.to_string()))
        }
    }
}

/// The previously known identity for `uri`, if the client sent one.
fn previous_identity<'a>(params: &'a WorkspaceParams, uri: &'a str) -> Option<&'a str> {
    params
        .previous_result_ids
        .iter()
        .find(|known| known.uri == uri)
        .map(|known| known.value.as_str())
}

/// Stamp one streamed or collected report with its document identity.
fn stamp_document(item: &mut Value, uri: &str, version: i32) -> Result<(), JsonRpcError> {
    let fields = item
        .as_object_mut()
        .ok_or_else(|| JsonRpcError::new(-32603, "report must be an object"))?;
    fields.insert("uri".to_owned(), json!(uri));
    fields.insert("version".to_owned(), json!(version));
    Ok(())
}

/// Stream one partial report while the reply stream is still open.
fn emit_partial(
    server: &Server,
    gate: &ReplyGate,
    token: &Value,
    item: &Value,
) -> Result<(), JsonRpcError> {
    let progress = serde_json::json!({ "token": token, "value": [item] });
    gate.emit(|| {
        server
            .outbox
            .send(lspkit_server::Message::notification("$/progress", progress))
    })
    .map_err(|_| JsonRpcError::new(-32803, "partial diagnostic stream closed"))
}

/// The cancellation error code every cancelled request reports.
pub(super) fn cancelled_error() -> JsonRpcError {
    JsonRpcError::new(-32800, "request cancelled")
}

/// Every scanned document: opened documents plus root JSON files, sorted.
fn scanned_documents(sources: &analysis::Sources) -> Result<Vec<String>, JsonRpcError> {
    let mut uris: Vec<String> = sources
        .documents
        .lock()
        .map(|documents| documents.keys().cloned().collect())
        .unwrap_or_default();
    if let Some(root) = sources.root.lock().ok().and_then(|root| root.clone()) {
        uris.extend(json_files(&root)?);
    }
    uris.sort();
    uris.dedup();
    Ok(uris)
}

/// File URIs of the JSON files directly inside `root`.
fn json_files(root: &std::path::Path) -> Result<Vec<String>, JsonRpcError> {
    let entries =
        std::fs::read_dir(root).map_err(|error| JsonRpcError::new(-32603, error.to_string()))?;
    let paths = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| JsonRpcError::new(-32603, error.to_string()))?;
    paths
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| {
            lspkit_server::path_to_uri(&path)
                .map_err(|error| JsonRpcError::new(-32603, error.to_string()))
        })
        .collect()
}
