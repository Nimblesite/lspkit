//! Rendered native consumer/producer sessions. [LSP-PULL-TRANSPORT]
//! The CLI owns the shared reader; tests observe its output and real process exit.

use serde_json::{json, Value};

use common::{at, entry};

mod common;
#[path = "../../../test-support/frame_events.rs"]
#[expect(
    clippy::expect_used,
    reason = "shared test-only module is outside Clippy's test-function detection"
)]
mod frame_events;

/// Send a complete native input stream and collect the CLI's rendered journal.
async fn client_events(input: &[u8], limit: Option<usize>) -> Vec<Value> {
    let limit = limit.map(|value| value.to_string());
    let args = limit
        .as_deref()
        .map_or_else(|| vec!["--client"], |limit| vec!["--client", limit]);
    frame_events::journal(&args, input).await
}

/// Serialize requests; the consumer, rather than the test, implements framed IPC.
fn requests(values: &[Value]) -> Result<Vec<u8>, serde_json::Error> {
    values.iter().try_fold(Vec::new(), |mut bytes, value| {
        serde_json::to_writer(&mut bytes, value)?;
        bytes.push(b'\n');
        Ok(bytes)
    })
}

/// Complete reader shutdown is observable once, after all response traffic.
fn assert_closed(events: &[Value]) {
    frame_events::assert_closed(events, Some(true));
}

/// The native reader's ordered response stream, independent of notifications.
fn responses(events: &[Value]) -> Vec<Value> {
    events
        .iter()
        .filter(|event| at(event, "/kind") == "response")
        .map(|event| at(event, "/envelope").clone())
        .collect()
}

/// Exact compiler location, severity, origin and message remain source-owned.
fn assert_real_parse_error(response: &Value) {
    let expected = match serde_json::from_str::<Value>("{\"unterminated\": ") {
        Err(error) => error,
        Ok(actual) => {
            assert_eq!(actual, Value::Null, "actual fixture is invalid JSON");
            return;
        }
    };
    let diagnostics = at(response, "/result/diagnostics").as_array();
    assert_eq!(diagnostics.map_or(0, Vec::len), 1);
    let diagnostic = diagnostics
        .and_then(|items| items.first())
        .unwrap_or(&Value::Null);
    assert_eq!(
        at(diagnostic, "/message").as_str().unwrap_or_default(),
        expected.to_string()
    );
    assert_eq!(at(diagnostic, "/range/start/line"), expected.line() - 1);
    assert_eq!(
        at(diagnostic, "/range/start/character"),
        expected.column() - 1
    );
    assert_eq!(at(diagnostic, "/severity"), 1);
    assert_eq!(at(diagnostic, "/source"), "serde_json");
}

#[tokio::test]
async fn idle_notifications_route_independently_of_responses() {
    let workspace = tempfile::TempDir::new().expect("tempdir");
    let broken = workspace.path().join("broken.json");
    std::fs::write(&broken, "{\"unterminated\": ").expect("write real fixture");
    let input = requests(&[
        json!({"id":1,"method":"initialize","params":{}}),
        json!({"id":2,"method":"analyze","params":{"path":broken}}),
        json!({"id":3,"method":"analyze","params":{"path":broken}}),
    ])
    .expect("serialize native requests");
    let events = client_events(&input, None).await;
    assert_ready_and_responses(&events);
    assert_closed(&events);
}

/// The idle ready event must not consume or reorder any response.
fn assert_ready_and_responses(events: &[Value]) {
    let ready: Vec<_> = events
        .iter()
        .filter(|event| at(event, "/kind") == "notification")
        .collect();
    assert_eq!(ready.len(), 1);
    let notification = ready.first().copied().unwrap_or(&Value::Null);
    assert_eq!(at(notification, "/envelope/method"), "ready");
    let replies = responses(events);
    assert_eq!(replies.len(), 3);
    assert_eq!(at(entry(&replies, 0), "/id"), 1);
    assert_eq!(
        at(entry(&replies, 0), "/result/protocol"),
        "lspkit-sidecar/1"
    );
    assert_eq!(at(entry(&replies, 1), "/id"), 2);
    assert_real_parse_error(entry(&replies, 1));
    assert_eq!(at(entry(&replies, 2), "/id"), 3);
    assert_real_parse_error(entry(&replies, 2));
    assert!(!events.iter().any(|event| at(event, "/kind") == "error"));
}

#[tokio::test]
async fn backend_io_failure_stays_an_honest_error() {
    let workspace = tempfile::TempDir::new().expect("tempdir");
    let missing = workspace.path().join("missing.json");
    let input = requests(&[json!({"id":7,"method":"analyze","params":{"path":missing}})])
        .expect("serialize native request");
    let events = client_events(&input, None).await;
    let replies = responses(&events);
    assert_eq!(replies.len(), 1);
    assert_eq!(at(entry(&replies, 0), "/id"), 7);
    assert!(at(entry(&replies, 0), "/error")
        .as_str()
        .unwrap_or_default()
        .contains("read failed"));
    assert_closed(&events);
}

/// A terminal codec/transport fault is visible and cannot masquerade as a response.
fn terminal_failure(events: &[Value]) -> &str {
    let errors: Vec<_> = events
        .iter()
        .filter(|event| at(event, "/kind") == "error")
        .collect();
    assert_eq!(errors.len(), 1);
    let error = errors.first().copied().unwrap_or(&Value::Null);
    let message = at(error, "/message").as_str().unwrap_or_default();
    assert_ne!(message, "");
    assert_eq!(responses(events), Vec::<Value>::new());
    assert_closed(events);
    message
}

#[tokio::test]
async fn codec_failure_is_terminal_and_visible() {
    let events = client_events(b"{ not valid json\n", None).await;
    let _message = terminal_failure(&events);
}

#[tokio::test]
async fn oversized_frame_is_refused() {
    let mut input = vec![7_u8; 16];
    input.push(b'\n');
    let events = client_events(&input, Some(8)).await;
    assert!(terminal_failure(&events).contains("frame too large"));
}

#[tokio::test]
async fn stdin_eof_ends_the_backend_cleanly() {
    let events = client_events(&[], None).await;
    assert_eq!(events.len(), 1);
    assert_eq!(responses(&events), Vec::<Value>::new());
    assert_closed(&events);
}

/// [LSP-PULL-CANCELLATION]: queued controls outlive neither their owner nor removed roots.
#[tokio::test]
async fn weak_ingress_controls_preserve_cancellation_and_release_workspace_ownership() {
    let first = tempfile::tempdir().expect("first real workspace");
    let second = tempfile::tempdir().expect("second real workspace");
    let args = [
        "--lifetime",
        first.path().to_str().expect("first workspace argument"),
        second.path().to_str().expect("second workspace argument"),
    ];
    let events = frame_events::journal(&args, &[]).await;
    assert_lifetime_terminal(&events);
    assert_lifetime_scope(entry(&events, 0), 2, true);
    assert_lifetime_scope(entry(&events, 1), 1, false);
}

/// Cancellation and release remain terminal, ordered, and ownership-aware.
fn assert_lifetime_terminal(events: &[Value]) {
    assert_eq!(
        events.len(),
        4,
        "two scopes, cancellation and owned release"
    );
    assert_eq!(at(entry(events, 2), "/kind"), "failure");
    assert_eq!(at(entry(events, 2), "/error/code"), -32800);
    assert!(entry(events, 2).get("result").is_none());
    assert_eq!(at(entry(events, 3), "/kind"), "released");
    assert_eq!(at(entry(events, 3), "/owner_released"), true);
    assert_eq!(at(entry(events, 3), "/late_inputs_accepted"), true);
}

/// Each retained scope exposes exactly the roots it still owns.
fn assert_lifetime_scope(event: &Value, count: usize, first_owned: bool) {
    assert_eq!(at(event, "/kind"), "snapshot");
    assert!(event.get("error").is_none());
    assert_eq!(at(event, "/result/count"), count);
    assert_eq!(at(event, "/result/first_owned"), first_owned);
    assert_eq!(at(event, "/result/second_owned"), true);
    let names = at(event, "/result/names")
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(names.len(), count);
    assert!(names.iter().any(|name| name == "second"));
    assert_eq!(names.iter().any(|name| name == "first"), first_owned);
}
