//! The LSP stdio session: read loop, dispatcher scheduling, capability
//! advertisement, and document pulls behind the shared memoized store.
//! [LSP-PULL-SALSA], [LSP-PULL-CANCELLATION]

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use lspkit_server::capabilities::decode_initialize;
use lspkit_server::jsonrpc::read_message;
use lspkit_server::pull::{PullError, PullReport, PullStore};
use lspkit_server::{
    CancellationToken, Capabilities, DispatchError, Dispatcher, Feature, FramingError,
    HandlerResult, JsonRpcError, Message, ReplyGate, RequestId, TextSync,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::BufReader;

use crate::analysis::{self, Document, Sources};
use crate::wire::{pump, Outbox};
use crate::{ingress, scan};

/// The workspace pull method, answered with streamed partial replies.
pub(super) const WORKSPACE_DIAGNOSTIC: &str = "workspace/diagnostic";

/// Shared per-connection state; clones observe one session.
#[derive(Clone)]
pub(super) struct Server {
    pub(super) sources: Sources,
    pub(super) pull: PullStore,
    dispatcher: Dispatcher,
    pub(super) gates: Arc<Mutex<HashMap<RequestId, ReplyGate>>>,
    pub(super) outbox: Outbox,
}

impl Server {
    fn new(outbox: Outbox) -> Self {
        let sources = Sources::default();
        let pull = PullStore::new(analysis::analyzer(sources.clone()));
        Self {
            sources,
            pull,
            dispatcher: Dispatcher::new(),
            gates: Arc::new(Mutex::new(HashMap::new())),
            outbox,
        }
    }

    /// Queue one outbound frame, logging egress failure.
    pub(super) fn send(&self, message: Message) {
        if let Err(egress) = self.outbox.send(message) {
            tracing::warn!(error = %egress, "reference egress failed");
        }
    }
}

/// Serve one client on stdio until `exit` or EOF; success on a clean end.
pub(super) async fn serve() -> std::process::ExitCode {
    let (inbox, receiver) = tokio::sync::mpsc::unbounded_channel();
    let outbox = Outbox::new(inbox);
    let server = Server::new(outbox.clone());
    register_handlers(&server);
    tokio::spawn(pump(
        receiver,
        lspkit_server::MessageWriter::new(tokio::io::stdout()),
        outbox.clone(),
    ));
    let outcome = read_loop(&server).await;
    outbox.quiesce().await;
    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error = %error, "reference session failed");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Read framed messages until `exit` or EOF. EOF is a clean shutdown.
async fn read_loop(server: &Server) -> Result<(), FramingError> {
    let mut stdin = BufReader::new(tokio::io::stdin());
    loop {
        let message = match read_message(&mut stdin).await {
            Ok(message) => message,
            Err(FramingError::Closed) => return Ok(()),
            Err(error) => return Err(error),
        };
        if handle_notification(server, &message) {
            return Ok(());
        }
        if let (Some(id), Some(method)) = (message.id.clone(), message.method.clone()) {
            // Dispatcher preparation registers cancellation before spawning work.
            schedule(server, id, &method, message.params.unwrap_or(Value::Null));
        }
    }
}

/// Handle one inbound notification; returns `true` only for `exit`.
fn handle_notification(server: &Server, message: &Message) -> bool {
    let Some(method) = message.method.as_deref() else {
        return false;
    };
    let params = message.params.clone().unwrap_or(Value::Null);
    if method == "exit" {
        server.dispatcher.cancel_all();
        return true;
    }
    match method {
        "$/cancelRequest" => cancel_request(server, &params),
        "textDocument/didOpen" => store_document(server, opened(&params)),
        "textDocument/didChange" => store_document(server, changed(&params)),
        "workspace/didChangeWatchedFiles" => {
            ingress::changed(&server.dispatcher.cancellation_handle(), &params);
            server.pull.invalidate();
        }
        _ => {}
    }
    false
}

/// Trip the cancellation token of the request named by `$/cancelRequest`.
fn cancel_request(server: &Server, params: &Value) {
    server
        .dispatcher
        .cancellation_handle()
        .cancel_request(params);
}

/// Store one synchronized document snapshot and invalidate the workspace.
fn store_document(server: &Server, document: Option<(String, Document)>) {
    let Some((uri, document)) = document else {
        tracing::warn!("malformed text synchronization notification ignored");
        return;
    };
    if let Ok(mut documents) = server.sources.documents.lock() {
        documents.insert(uri, document);
    }
    server.pull.invalidate();
}

/// A full document supplied by `didOpen`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Opened {
    text_document: OpenedText,
}

/// The synchronized text and version supplied by the editor.
#[derive(Deserialize)]
struct OpenedText {
    uri: String,
    version: i32,
    text: String,
}

/// Decode a `didOpen` into a document snapshot.
fn opened(params: &Value) -> Option<(String, Document)> {
    let params = serde_json::from_value::<Opened>(params.clone()).ok()?;
    let text = params.text_document;
    Some((
        text.uri,
        Document {
            text: text.text,
            version: text.version,
        },
    ))
}

/// The existing full-sync protocol input, decoded once at the boundary.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Change {
    text_document: ChangedText,
    content_changes: Vec<FullChange>,
}

#[derive(Deserialize)]
struct ChangedText {
    uri: String,
    version: i32,
}

#[derive(Deserialize)]
struct FullChange {
    text: String,
}

/// Decode a full-sync `didChange` into a document snapshot.
fn changed(params: &Value) -> Option<(String, Document)> {
    let params = serde_json::from_value::<Change>(params.clone()).ok()?;
    let change = params.content_changes.last()?;
    Some((
        params.text_document.uri,
        Document {
            text: change.text.clone(),
            version: params.text_document.version,
        },
    ))
}

/// Register the request handlers every reference session serves.
fn register_handlers(server: &Server) {
    let init = server.clone();
    server.dispatcher.register("initialize", move |params, _| {
        let init = init.clone();
        async move { initialize(&init, params) }
    });
    let documents = server.clone();
    server
        .dispatcher
        .register("textDocument/diagnostic", move |params, cancel| {
            document_query(documents.clone(), params, cancel)
        });
    let work = server.clone();
    server.dispatcher.register("reference/work", move |_, _| {
        let work = work.clone();
        async move { HandlerResult::Ok(work_report(&work)) }
    });
    server
        .dispatcher
        .register("shutdown", |_, _| async { HandlerResult::Ok(Value::Null) });
}

/// Supporting metadata: backend work count and the current revision.
fn work_report(server: &Server) -> Value {
    json!({
        "analysisRuns": server.sources.analysis_runs(),
        "revision": server.pull.revision(),
    })
}

/// Decode initialize capabilities and start watching the workspace root.
fn initialize(server: &Server, params: Value) -> HandlerResult {
    let Ok(init) = decode_initialize(params) else {
        return HandlerResult::Err(JsonRpcError::new(-32602, "malformed initialize params"));
    };
    if let Some(root) = analysis::root_path(&init) {
        let sources = server.sources.clone();
        let invalidating = server.pull.clone();
        let outbox = server.outbox.clone();
        analysis::watch_root(&sources, &root, move || {
            invalidating.invalidate();
            let event = Message::notification("$/referenceWorkspaceChanged", json!({}));
            if let Err(error) = outbox.send(event) {
                tracing::warn!(%error, "reference watcher notification failed");
            }
        });
    }
    let capabilities = Capabilities::new()
        .text_sync(TextSync::Full)
        .enable(Feature::DiagnosticsPull);
    HandlerResult::Ok(advertise(&capabilities))
}

/// Advertise the negotiated server capabilities.
fn advertise(caps: &Capabilities) -> Value {
    let provider = json!({
        "identifier": "diagnostic-reference",
        "interFileDependencies": true,
        "workspaceDiagnostics": true,
    });
    let provider = if caps.has(Feature::DiagnosticsPull) {
        provider
    } else {
        Value::Null
    };
    json!({
        "capabilities": {
            "positionEncoding": "utf-16",
            "textDocumentSync": sync_kind(caps.text_sync_kind()),
            "diagnosticProvider": provider,
        }
    })
}

/// LSP text-document-sync kind number.
fn sync_kind(kind: TextSync) -> u8 {
    match kind {
        TextSync::None => 0,
        TextSync::Full => 1,
        _ => 2,
    }
}

/// Schedule a request; the workspace scan streams through a reply gate.
fn schedule(server: &Server, id: RequestId, method: &str, params: Value) {
    if let Err(error) = ingress::register(&server.dispatcher, id.clone(), method, &params) {
        respond_error(server, id, JsonRpcError::new(-32603, error.to_string()));
        return;
    }
    if method == WORKSPACE_DIAGNOSTIC {
        schedule_scan(server, id, params);
    } else {
        schedule_request(server, id, method, params);
    }
}

/// Schedule an ordinary handler-backed request.
fn schedule_request(server: &Server, id: RequestId, method: &str, params: Value) {
    match server.dispatcher.prepare(id.clone(), method, params) {
        Ok(future) => {
            server.outbox.begin_request();
            drop(tokio::spawn(respond(server.clone(), id, future)));
        }
        Err(DispatchError::UnknownMethod(unknown)) => respond_error(
            server,
            id,
            JsonRpcError::new(-32601, format!("method not found: {unknown}")),
        ),
        Err(error) => respond_error(server, id, JsonRpcError::new(-32603, error.to_string())),
    }
}

/// Schedule the workspace scan behind a per-request reply gate.
fn schedule_scan(server: &Server, id: RequestId, params: Value) {
    let gate = ReplyGate::default();
    let scan_server = server.clone();
    let scan_gate = gate.clone();
    let scheduled = server.dispatcher.prepare_call(id.clone(), move |cancel| {
        let server = scan_server;
        let gate = scan_gate;
        async move { scan::run_workspace_scan(&server, &gate, params, &cancel).await }
    });
    match scheduled {
        Ok(future) => {
            remember_gate(server, &id, gate);
            server.outbox.begin_request();
            drop(tokio::spawn(respond(server.clone(), id, future)));
        }
        Err(error) => respond_error(server, id, JsonRpcError::new(-32603, error.to_string())),
    }
}

/// Keep the scan's reply gate so the terminal reply closes the stream.
fn remember_gate(server: &Server, id: &RequestId, gate: ReplyGate) {
    if let Ok(mut gates) = server.gates.lock() {
        gates.insert(id.clone(), gate);
    }
}

/// Answer a request that was never scheduled.
fn respond_error(server: &Server, id: RequestId, error: JsonRpcError) {
    server.send(Message::response_error(id, error));
}

/// Await a request and emit its terminal reply through any remembered gate.
async fn respond(server: Server, id: RequestId, future: impl Future<Output = HandlerResult>) {
    let result = future.await;
    let gate = server
        .gates
        .lock()
        .ok()
        .and_then(|mut gates| gates.remove(&id));
    let message = match result {
        HandlerResult::Ok(value) => Message::response(id, value),
        HandlerResult::Err(error) => Message::response_error(id, error),
        // `HandlerResult` is `#[non_exhaustive]`; an outcome without a wire
        // form is reported honestly instead of fabricating success.
        _ => Message::response_error(id, JsonRpcError::new(-32603, "unsupported handler outcome")),
    };
    emit_terminal(&server, gate, message);
    server.outbox.finish_request();
}

/// The same terminal emission closes every remembered partial stream.
fn emit_terminal(server: &Server, gate: Option<ReplyGate>, message: Message) {
    match gate {
        Some(gate) => {
            if let Err(emission) = gate.finish(|| server.outbox.send(message)) {
                tracing::warn!(error = %emission, "reference terminal reply failed");
            }
        }
        None => server.send(message),
    }
}

/// Native/compiler work must never block the foreground protocol reader.
async fn document_query(server: Server, raw: Value, cancel: CancellationToken) -> HandlerResult {
    let params = match DocumentDiagnostic::decode(raw) {
        Ok(params) => params,
        Err(error) => return HandlerResult::Err(error),
    };
    let version = params.version(&server.sources);
    let result_id = server.pull.result_id(version);
    if !cancel.is_cancelled() && params.previous_result_id.as_deref() == Some(result_id.as_str()) {
        return HandlerResult::Ok(unchanged_report(&result_id));
    }
    match tokio::task::spawn_blocking(move || document_pull(&server, &params, version, &cancel))
        .await
    {
        Ok(result) => result,
        Err(error) => HandlerResult::Err(JsonRpcError::new(-32603, error.to_string())),
    }
}

/// Answer one document pull from the shared memoized store.
fn document_pull(
    server: &Server,
    params: &DocumentDiagnostic,
    version: i32,
    cancel: &CancellationToken,
) -> HandlerResult {
    match server
        .pull
        .diagnostics_cancellable(&params.text_document.uri, version, cancel)
    {
        Ok(report) => match document_report(&report, params.previous_result_id.as_deref()) {
            Ok(value) => HandlerResult::Ok(value),
            Err(error) => HandlerResult::Err(error),
        },
        Err(PullError::Cancelled) => HandlerResult::Err(scan::cancelled_error()),
        Err(error) => HandlerResult::Err(JsonRpcError::new(-32603, error.to_string())),
    }
}

/// `textDocument/diagnostic` request parameters.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentDiagnostic {
    text_document: TextDocumentRef,
    previous_result_id: Option<String>,
}

impl DocumentDiagnostic {
    /// Decode at the foreground boundary before compiler work is scheduled.
    fn decode(raw: Value) -> Result<Self, JsonRpcError> {
        serde_json::from_value(raw)
            .map_err(|_| JsonRpcError::new(-32602, "malformed textDocument/diagnostic params"))
    }

    /// A supplied version or the current synchronized editor input.
    fn version(&self, sources: &Sources) -> i32 {
        self.text_document
            .version
            .unwrap_or_else(|| analysis::document_version(sources, &self.text_document.uri))
    }
}

/// The text-document reference inside a pull request.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextDocumentRef {
    uri: String,
    version: Option<i32>,
}

/// Full truth for a new identity; `unchanged` for a repeated one.
pub(super) fn document_report(
    report: &PullReport,
    previous: Option<&str>,
) -> Result<Value, JsonRpcError> {
    if previous == Some(report.result_id.as_str()) {
        return Ok(unchanged_report(&report.result_id));
    }
    let items = serde_json::to_value(report.items.as_ref())
        .map_err(|error| JsonRpcError::new(-32603, error.to_string()))?;
    Ok(json!({ "kind": "full", "resultId": report.result_id, "items": items }))
}

/// Document and workspace responses use one unchanged wire representation.
fn unchanged_report(result_id: &str) -> Value {
    json!({ "kind": "unchanged", "resultId": result_id })
}
