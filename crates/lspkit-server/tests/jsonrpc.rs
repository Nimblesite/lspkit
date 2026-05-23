//! End-to-end tests for [`lspkit_server::jsonrpc`] framing and types.

use lspkit_server::{JsonRpcError, Message, MessageWriter, RequestId};
use serde_json::{json, Value};
use tokio::io::BufReader;

#[tokio::test]
async fn write_and_read_round_trip_request() {
    let original = Message::request(RequestId::Number(1), "initialize", json!({"x": 1}));
    let mut buffer: Vec<u8> = Vec::new();
    {
        let writer = MessageWriter::new(&mut buffer);
        writer.write_message(&original).await.expect("write ok");
    }

    assert!(!buffer.is_empty(), "writer must emit at least the header");
    let on_wire = std::str::from_utf8(&buffer).expect("framed message is utf-8");
    assert!(
        on_wire.starts_with("Content-Length: "),
        "frame must start with Content-Length header"
    );
    assert!(
        on_wire.contains("\r\n\r\n"),
        "header/body delimiter required"
    );
    assert!(on_wire.contains("\"method\":\"initialize\""));

    let cursor = std::io::Cursor::new(buffer);
    let mut reader = BufReader::new(cursor);
    let decoded = lspkit_server::jsonrpc::read_message(&mut reader)
        .await
        .expect("read ok");
    assert_eq!(decoded.jsonrpc, "2.0");
    assert_eq!(decoded.method.as_deref(), Some("initialize"));
    assert_eq!(decoded.id, Some(RequestId::Number(1)));
    assert_eq!(decoded.params, Some(json!({"x": 1})));
    assert!(decoded.result.is_none(), "requests carry no result");
    assert!(decoded.error.is_none(), "requests carry no error");
}

#[tokio::test]
async fn write_and_read_notification() {
    let notif = Message::notification("textDocument/didChange", json!({"version": 2}));
    let mut buffer: Vec<u8> = Vec::new();
    {
        let writer = MessageWriter::new(&mut buffer);
        writer.write_message(&notif).await.expect("write ok");
    }

    let on_wire = std::str::from_utf8(&buffer).expect("utf-8");
    assert!(on_wire.contains("Content-Length:"), "header present");
    assert!(
        on_wire.contains("textDocument/didChange"),
        "method appears on wire"
    );
    assert!(
        !on_wire.contains("\"id\":"),
        "notifications must not serialize an id"
    );

    let cursor = std::io::Cursor::new(buffer);
    let mut reader = BufReader::new(cursor);
    let decoded = lspkit_server::jsonrpc::read_message(&mut reader)
        .await
        .expect("read ok");
    assert_eq!(decoded.jsonrpc, "2.0");
    assert!(decoded.id.is_none(), "notifications have no id");
    assert_eq!(decoded.method.as_deref(), Some("textDocument/didChange"));
    assert_eq!(decoded.params, Some(json!({"version": 2})));
    assert!(decoded.result.is_none());
    assert!(decoded.error.is_none());
}

#[tokio::test]
async fn write_and_read_response_success() {
    let response = Message::response(RequestId::String("req-1".to_owned()), json!({"ok": true}));
    let mut buffer: Vec<u8> = Vec::new();
    {
        let writer = MessageWriter::new(&mut buffer);
        writer.write_message(&response).await.expect("write ok");
    }

    assert!(!buffer.is_empty());
    let on_wire = std::str::from_utf8(&buffer).expect("utf-8");
    assert!(
        on_wire.contains("\"id\":\"req-1\""),
        "string id serialized as quoted JSON string"
    );
    assert!(
        on_wire.contains("\"result\":"),
        "result field present on wire"
    );
    assert!(
        !on_wire.contains("\"error\":"),
        "success response must not carry error"
    );

    let cursor = std::io::Cursor::new(buffer);
    let mut reader = BufReader::new(cursor);
    let decoded = lspkit_server::jsonrpc::read_message(&mut reader)
        .await
        .expect("read ok");
    assert_eq!(decoded.jsonrpc, "2.0");
    assert_eq!(decoded.id, Some(RequestId::String("req-1".to_owned())));
    assert_eq!(decoded.result, Some(json!({"ok": true})));
    assert!(decoded.error.is_none(), "success response has no error");
    assert!(decoded.method.is_none(), "responses have no method");
    assert!(decoded.params.is_none(), "responses have no params");
}

#[tokio::test]
async fn write_and_read_response_error() {
    let err = JsonRpcError::new(-32601, "Method not found").with_data(json!({"hint": "x"}));
    let response = Message::response_error(RequestId::Number(7), err);
    let mut buffer: Vec<u8> = Vec::new();
    {
        let writer = MessageWriter::new(&mut buffer);
        writer.write_message(&response).await.expect("write ok");
    }

    let on_wire = std::str::from_utf8(&buffer).expect("utf-8");
    assert!(
        on_wire.contains("\"id\":7"),
        "numeric id serialized as bare integer"
    );
    assert!(on_wire.contains("\"code\":-32601"));
    assert!(on_wire.contains("Method not found"));

    let cursor = std::io::Cursor::new(buffer);
    let mut reader = BufReader::new(cursor);
    let decoded = lspkit_server::jsonrpc::read_message(&mut reader)
        .await
        .expect("read ok");
    assert_eq!(decoded.jsonrpc, "2.0");
    assert_eq!(decoded.id, Some(RequestId::Number(7)));
    assert!(decoded.result.is_none(), "error response carries no result");
    assert!(decoded.method.is_none());
    assert!(decoded.params.is_none());
    let received = decoded.error.expect("error present");
    assert_eq!(received.code, -32601);
    assert_eq!(received.message, "Method not found");
    assert_eq!(received.data, Some(json!({"hint": "x"})));
}

#[tokio::test]
async fn read_message_rejects_missing_content_length() {
    let raw = b"X-Other: 1\r\n\r\n{}".to_vec();
    let cursor = std::io::Cursor::new(raw);
    let mut reader = BufReader::new(cursor);
    let err = lspkit_server::jsonrpc::read_message(&mut reader)
        .await
        .expect_err("must reject");
    let rendered = format!("{err}");
    assert!(rendered.contains("Content-Length") || rendered.contains("missing"));
}

#[tokio::test]
async fn read_message_rejects_bad_content_length_value() {
    let raw = b"Content-Length: not-a-number\r\n\r\n{}".to_vec();
    let cursor = std::io::Cursor::new(raw);
    let mut reader = BufReader::new(cursor);
    let err = lspkit_server::jsonrpc::read_message(&mut reader)
        .await
        .expect_err("must reject");
    assert!(format!("{err}").contains("Content-Length"));
}

#[tokio::test]
async fn read_message_rejects_invalid_json_body() {
    let body = b"not json";
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut raw = header.into_bytes();
    raw.extend_from_slice(body);
    let cursor = std::io::Cursor::new(raw);
    let mut reader = BufReader::new(cursor);
    let err = lspkit_server::jsonrpc::read_message(&mut reader)
        .await
        .expect_err("must reject");
    assert!(format!("{err}").contains("JSON"));
}

#[tokio::test]
async fn read_message_reports_closed_stream() {
    let cursor = std::io::Cursor::new(Vec::<u8>::new());
    let mut reader = BufReader::new(cursor);
    let err = lspkit_server::jsonrpc::read_message(&mut reader)
        .await
        .expect_err("must reject");
    assert!(format!("{err}").contains("closed"));
}

#[tokio::test]
async fn read_message_tolerates_extra_headers() {
    let body = serde_json::to_vec(&json!({"jsonrpc": "2.0", "method": "ping"})).expect("ok");
    let mut raw = format!(
        "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    raw.extend_from_slice(&body);
    let cursor = std::io::Cursor::new(raw);
    let mut reader = BufReader::new(cursor);
    let decoded = lspkit_server::jsonrpc::read_message(&mut reader)
        .await
        .expect("read ok");
    assert_eq!(decoded.method.as_deref(), Some("ping"));
}

#[tokio::test]
async fn writer_serializes_two_messages_back_to_back() {
    let mut buffer: Vec<u8> = Vec::new();
    {
        let writer = MessageWriter::new(&mut buffer);
        writer
            .write_message(&Message::notification("a", json!({"i": 0})))
            .await
            .expect("ok");
        writer
            .write_message(&Message::notification("b", json!({"i": 1})))
            .await
            .expect("ok");
    }

    let on_wire = std::str::from_utf8(&buffer).expect("utf-8");
    let header_count = on_wire.matches("Content-Length:").count();
    assert_eq!(header_count, 2, "exactly two frames written");
    let delimiter_count = on_wire.matches("\r\n\r\n").count();
    assert_eq!(delimiter_count, 2, "each frame has a header/body delimiter");

    let cursor = std::io::Cursor::new(buffer);
    let mut reader = BufReader::new(cursor);
    let first = lspkit_server::jsonrpc::read_message(&mut reader)
        .await
        .expect("first");
    let second = lspkit_server::jsonrpc::read_message(&mut reader)
        .await
        .expect("second");
    assert_eq!(first.method.as_deref(), Some("a"));
    assert_eq!(second.method.as_deref(), Some("b"));
    assert_eq!(first.jsonrpc, "2.0");
    assert_eq!(second.jsonrpc, "2.0");
    assert!(
        first.id.is_none() && second.id.is_none(),
        "notifications carry no id"
    );
    assert_eq!(first.params, Some(json!({"i": 0})));
    assert_eq!(second.params, Some(json!({"i": 1})));
    assert!(first.result.is_none() && first.error.is_none());
    assert!(second.result.is_none() && second.error.is_none());

    let trailing = lspkit_server::jsonrpc::read_message(&mut reader).await;
    assert!(trailing.is_err(), "no third frame on the wire");
}

#[test]
fn request_id_number_serialization_round_trip() {
    let id = RequestId::Number(42);
    let encoded = serde_json::to_string(&id).expect("ok");
    assert_eq!(encoded, "42");
    let decoded: RequestId = serde_json::from_str(&encoded).expect("ok");
    assert_eq!(decoded, id);
}

#[test]
fn request_id_string_serialization_round_trip() {
    let id = RequestId::String("abc".to_owned());
    let encoded = serde_json::to_string(&id).expect("ok");
    assert_eq!(encoded, "\"abc\"");
    let decoded: RequestId = serde_json::from_str(&encoded).expect("ok");
    assert_eq!(decoded, id);
}

#[test]
fn request_id_clone_and_eq_work() {
    let id = RequestId::Number(1);
    let copy = id.clone();
    assert_eq!(id, copy);
    assert_ne!(id, RequestId::Number(2));
}

#[test]
fn jsonrpc_error_new_and_with_data() {
    let err = JsonRpcError::new(-32000, "boom");
    assert!(err.data.is_none());
    let with = err.with_data(json!({"x": 1}));
    assert_eq!(with.data, Some(json!({"x": 1})));
}

#[test]
fn jsonrpc_error_clone_and_debug_work() {
    let err = JsonRpcError::new(-1, "test");
    let copy = err.clone();
    assert_eq!(copy.code, -1);
    assert!(format!("{err:?}").contains("test"));
}

#[test]
fn framing_error_io_variant_via_from() {
    let io_err = std::io::Error::other("test");
    let framing: lspkit_server::FramingError = io_err.into();
    assert!(!format!("{framing}").is_empty());
}

#[test]
fn message_debug_works() {
    let msg = Message::notification("ping", Value::Null);
    assert!(format!("{msg:?}").contains("Message"));
}

#[test]
fn message_clone_preserves_fields() {
    let msg = Message::request(RequestId::Number(3), "m", json!(1));
    let cloned = msg.clone();
    assert_eq!(cloned.method, msg.method);
    assert_eq!(cloned.id, msg.id);
}
