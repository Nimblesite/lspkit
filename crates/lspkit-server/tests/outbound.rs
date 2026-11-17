//! [LSP-OUTBOUND] Server→client requests over real Content-Length framing:
//! the server allocates the id, the editor answers on the wire, and the
//! decoded response resolves the waiting caller. Abandoned waits unregister.

use lspkit_server::jsonrpc::read_message;
use lspkit_server::{
    JsonRpcError, Message, MessageWriter, Outbound, OutboundError, OutboundResponse, RequestId,
};
use serde_json::{json, Value};
use tokio::io::{duplex, BufReader, DuplexStream};

/// A wire step that fails the session with its cause.
type StepResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Enough buffer for every frame these sessions exchange.
const PIPE_BYTES: usize = 64 * 1024;

/// One direction of the editor connection, framed exactly as on stdio.
struct Wire {
    writer: MessageWriter<DuplexStream>,
    reader: BufReader<DuplexStream>,
}

/// A connected server/editor pair.
fn connection() -> (Wire, Wire) {
    let (server_out, editor_in) = duplex(PIPE_BYTES);
    let (editor_out, server_in) = duplex(PIPE_BYTES);
    (
        Wire {
            writer: MessageWriter::new(server_out),
            reader: BufReader::new(server_in),
        },
        Wire {
            writer: MessageWriter::new(editor_out),
            reader: BufReader::new(editor_in),
        },
    )
}

/// The editor reads one request and asserts its shape.
async fn editor_receives(editor: &mut Wire, method: &str, params: &Value) -> StepResult<RequestId> {
    let request = read_message(&mut editor.reader).await?;
    assert_eq!(request.jsonrpc, "2.0");
    assert_eq!(request.method.as_deref(), Some(method));
    // `"params": null` decodes as absent; both mean "no params" on the wire.
    assert_eq!(request.params.as_ref().unwrap_or(&Value::Null), params);
    let id = request.id.ok_or("a server request carries an id")?;
    assert!(matches!(id, RequestId::Number(raw) if raw > 0), "{id:?}");
    Ok(id)
}

/// The server sends `method`, and the editor reads exactly what was sent.
async fn round_trip(
    outbound: &Outbound,
    server: &Wire,
    editor: &mut Wire,
    method: &str,
    params: Value,
) -> StepResult<(RequestId, OutboundResponse)> {
    let (message, pending) = outbound.request(method, params.clone());
    server.writer.write_message(&message).await?;
    let id = editor_receives(editor, method, &params).await?;
    assert_eq!(
        message.id.as_ref(),
        Some(&id),
        "the wire carries the allocated id"
    );
    Ok((id, pending))
}

/// The editor writes `reply`; the server reads it off the wire and resolves it.
async fn answer(
    outbound: &Outbound,
    server: &mut Wire,
    editor: &Wire,
    reply: &Message,
) -> StepResult<Result<(), OutboundError>> {
    editor.writer.write_message(reply).await?;
    Ok(outbound.resolve(read_message(&mut server.reader).await?))
}

/// [LSP-OUTBOUND] One editor session answering with a result, `null` and an error.
#[tokio::test]
async fn server_requests_resolve_from_the_editors_wire_answers() {
    let (mut server, mut editor) = connection();
    let outbound = Outbound::new();
    assert_eq!(outbound.pending_count(), 0);

    // Success: window/showDocument answered with a result.
    let show = json!({ "uri": "file:///tmp/a.rs", "takeFocus": true });
    let (show_id, show_wait) =
        round_trip(&outbound, &server, &mut editor, "window/showDocument", show)
            .await
            .expect("request reaches the editor");
    assert_eq!(outbound.pending_count(), 1);
    let shown = Message::response(show_id.clone(), json!({ "success": true }));
    answer(&outbound, &mut server, &editor, &shown)
        .await
        .expect("wire")
        .expect("answer matches");
    let shown = show_wait
        .await
        .expect("delivered")
        .expect("the editor showed it");
    assert_eq!(shown, json!({ "success": true }));
    assert_eq!(outbound.pending_count(), 0);

    // A `null` result (workspace/diagnostic/refresh) resolves to Value::Null.
    let refresh = "workspace/diagnostic/refresh";
    let (refresh_id, refresh_wait) =
        round_trip(&outbound, &server, &mut editor, refresh, Value::Null)
            .await
            .expect("request reaches the editor");
    assert_ne!(refresh_id, show_id, "ids are never reused");
    let null = Message::response(refresh_id, Value::Null);
    answer(&outbound, &mut server, &editor, &null)
        .await
        .expect("wire")
        .expect("null matches");
    assert_eq!(
        refresh_wait
            .await
            .expect("delivered")
            .expect("null is success"),
        Value::Null
    );

    // Error: the editor's JsonRpcError reaches the caller, and it is a std error.
    let missing = json!({ "uri": "file:///missing" });
    let (error_id, error_wait) = round_trip(
        &outbound,
        &server,
        &mut editor,
        "window/showDocument",
        missing,
    )
    .await
    .expect("request reaches the editor");
    let refusal =
        JsonRpcError::new(-32603, "document refused").with_data(json!({ "why": "closed" }));
    let refused = Message::response_error(error_id, refusal);
    answer(&outbound, &mut server, &editor, &refused)
        .await
        .expect("wire")
        .expect("error matches");
    let error = error_wait
        .await
        .expect("delivered")
        .expect_err("the editor refused");
    assert_eq!(
        (error.code, error.message.as_str()),
        (-32603, "document refused")
    );
    assert_eq!(error.data, Some(json!({ "why": "closed" })));
    assert_eq!(error.to_string(), "JSON-RPC error -32603: document refused");
    let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(error);
    assert_eq!(boxed.to_string(), "JSON-RPC error -32603: document refused");
    assert_eq!(outbound.pending_count(), 0);
}

/// [LSP-OUTBOUND] [SIDECAR-REQUEST-LIFETIME] Abandoned waits leave no live entry,
/// and answers nobody waits for are rejected visibly, never dropped silently.
#[tokio::test]
async fn abandoned_waits_unregister_and_unmatched_messages_are_rejected() {
    let (mut server, mut editor) = connection();
    let outbound = Outbound::default();
    assert_eq!(outbound.pending_count(), 0, "a default handle starts empty");
    let late_params = json!({ "uri": "file:///late" });
    let (late_id, late_wait) = round_trip(
        &outbound,
        &server,
        &mut editor,
        "window/showDocument",
        late_params,
    )
    .await
    .expect("request reaches the editor");
    assert_eq!(outbound.pending_count(), 1);
    drop(late_wait);
    assert_eq!(
        outbound.pending_count(),
        0,
        "an abandoned wait leaves no live entry"
    );
    let late_reply = Message::response(late_id.clone(), json!({ "success": true }));
    let late = answer(&outbound, &mut server, &editor, &late_reply)
        .await
        .expect("wire")
        .expect_err("nobody waits");
    assert!(
        matches!(&late, OutboundError::UnknownId(id) if *id == late_id),
        "{late:?}"
    );
    assert_eq!(
        late.to_string(),
        format!("no pending server request for id {late_id:?}")
    );

    // Ids the server never allocated, and non-responses, are rejected visibly.
    for foreign in [
        RequestId::String("editor-7".to_owned()),
        RequestId::Number(-1),
        RequestId::Number(9_999),
    ] {
        let rejected = outbound
            .resolve(Message::response(foreign.clone(), Value::Null))
            .expect_err("foreign id");
        assert!(
            matches!(&rejected, OutboundError::UnknownId(id) if *id == foreign),
            "{rejected:?}"
        );
    }
    let notification = outbound
        .resolve(Message::notification("$/progress", json!({})))
        .expect_err("not a response");
    assert!(matches!(notification, OutboundError::NotAResponse));
    let hover = Message::request(RequestId::Number(1), "textDocument/hover", json!({}));
    let request = outbound.resolve(hover).expect_err("a request");
    assert!(matches!(request, OutboundError::NotAResponse));
    assert_eq!(request.to_string(), "message is not a response");
    assert_eq!(outbound.pending_count(), 0);
}
