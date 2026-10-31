//! `--backend` mode: a real out-of-process backend speaking the sidecar wire
//! protocol — 4-byte little-endian length-prefixed JSON frames over OS stdio
//! — for `lspkit`'s `tests/sidecar_inbound_routing.rs`. Requests are natural
//! backend calls; diagnostics come from real `serde_json` parses of real
//! files ([LSP-PULL-SALSA]).

use std::process::ExitCode;

use lspkit::sidecar::{TransportError, DEFAULT_MAX_FRAME};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Protocol serialization and canonical transport failures remain visible.
#[derive(Debug, thiserror::Error)]
enum BackendError {
    /// The shared framing layer rejected an incomplete or invalid frame.
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// A protocol value could not be serialized.
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
}

/// Serve framed requests until stdin EOF; success on a clean end.
pub(super) async fn serve() -> ExitCode {
    match serve_frames().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "reference backend session failed");
            ExitCode::FAILURE
        }
    }
}

/// Every packet uses the same shared framing implementation as real consumers.
async fn serve_frames() -> Result<(), BackendError> {
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut writer = tokio::io::stdout();
    loop {
        let Some(frame) = next_frame(&mut reader).await? else {
            return Ok(());
        };
        for envelope in handle(&frame)? {
            write_envelope(&mut writer, &envelope).await?;
        }
    }
}

/// Distinguish a clean empty stream before delegating to the shared decoder.
async fn next_frame(
    reader: &mut BufReader<tokio::io::Stdin>,
) -> Result<Option<Vec<u8>>, TransportError> {
    if reader.fill_buf().await?.is_empty() {
        return Ok(None);
    }
    lspkit::sidecar::read_frame(reader, DEFAULT_MAX_FRAME)
        .await
        .map(Some)
}

/// Serialize one envelope without duplicating framing or panicking.
async fn write_envelope(
    writer: &mut tokio::io::Stdout,
    envelope: &Value,
) -> Result<(), BackendError> {
    let body = serde_json::to_vec(envelope)?;
    lspkit::sidecar::write_frame(writer, &body).await?;
    Ok(())
}

/// The reply envelopes for one inbound frame, in emission order.
fn handle(frame: &[u8]) -> Result<Vec<Value>, BackendError> {
    let Ok(envelope) = serde_json::from_slice::<Value>(frame) else {
        return Ok(vec![
            json!({ "error": "decode failed: frame was not valid JSON" }),
        ]);
    };
    match envelope_request(&envelope) {
        Some((id, method, params)) => request_replies(&id, &method, &params),
        None => Ok(Vec::new()), // notifications are accepted and ignored
    }
}

/// Split an envelope into its request identity, method and params.
fn envelope_request(envelope: &Value) -> Option<(Value, String, Value)> {
    let id = envelope.get("id")?.clone();
    let method = envelope.get("method")?.as_str()?.to_owned();
    let params = envelope.get("params").cloned().unwrap_or(Value::Null);
    Some((id, method, params))
}

/// Natural backend calls: handshake with a ready event, real file analysis.
fn request_replies(id: &Value, method: &str, params: &Value) -> Result<Vec<Value>, BackendError> {
    match method {
        "initialize" => Ok(vec![
            json!({ "id": id, "result": {
                "protocol": "lspkit-sidecar/1", "backend": "diagnostic-reference" } }),
            json!({ "method": "ready", "params": { "backend": "diagnostic-reference" } }),
        ]),
        "analyze" => Ok(vec![analyze_reply(id, params)?]),
        other => Ok(vec![json!({
            "id": id, "error": format!("method not found: {other}")
        })]),
    }
}

/// Analyze the real file at `params.path`; IO failures stay honest errors.
fn analyze_reply(id: &Value, params: &Value) -> Result<Value, BackendError> {
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Ok(json!({ "id": id, "error": "malformed analyze params: missing path" }));
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            return Ok(json!({ "id": id, "error": format!("document read failed: {error}") }));
        }
    };
    let diagnostics = serde_json::to_value(crate::analysis::syntax_diagnostics(&text))?;
    Ok(json!({ "id": id, "result": { "path": path, "diagnostics": diagnostics } }))
}
