//! Rendered native disconnect sessions against real compiled programs.
//! [LSP-PULL-TRANSPORT]: no test invokes the shared reader or a fake backend.

use serde_json::{json, Value};

#[path = "../../../test-support/frame_events.rs"]
mod frame_events;

/// Inspect an actual captured native stream through the file-input CLI.
async fn received(bytes: &[u8]) -> Vec<Value> {
    let fixture = tempfile::TempDir::new().expect("native capture fixture");
    let path = fixture.path().join("stream.bin");
    std::fs::write(&path, bytes).expect("write captured native bytes");
    frame_events::journal(&["--receive", path.to_str().expect("capture path")], &[]).await
}

/// A source file parsed by the actual backend produces the wire fixture.
async fn produced_frame() -> (Vec<u8>, Value) {
    let fixture = tempfile::TempDir::new().expect("native source fixture");
    let path = fixture.path().join("valid.json");
    std::fs::write(&path, b"{\"answer\":42}").expect("real valid source");
    let request = json!({"id":7,"method":"analyze","params":{"path":path}});
    let payload = serde_json::to_vec(&request).expect("native request");
    let length = u32::try_from(payload.len())
        .expect("small request")
        .to_le_bytes();
    let input = [length.as_slice(), payload.as_slice()].concat();
    let output = frame_events::output(&["--backend"], &input).await;
    assert!(
        output.stdout.len() > 5,
        "actual backend emitted a complete frame"
    );
    (
        output.stdout,
        json!({"id":7,"result":{"path":path,"diagnostics":[]}}),
    )
}

/// A corrupt native stream produces one visible error, then releases both streams.
fn assert_failure(events: &[Value]) {
    let failures: Vec<_> = events
        .iter()
        .filter(|event| event["kind"] == "error")
        .collect();
    assert_eq!(failures.len(), 1, "one terminal native read error");
    assert!(!failures[0]["message"]
        .as_str()
        .expect("visible read failure")
        .is_empty());
    frame_events::assert_closed(events, None);
}

/// A torn prefix or body never becomes a successful response or clean EOF.
async fn assert_truncation(bytes: &[u8]) {
    let events = received(bytes).await;
    assert_failure(&events);
    assert_eq!(
        events.len(),
        2,
        "failure followed by terminal ownership release"
    );
    assert_eq!(events[0]["kind"], "error");
}

#[tokio::test]
async fn native_disconnects_preserve_prefix_and_payload_failures_after_valid_traffic() {
    let empty = frame_events::output(&["--backend"], &[]).await.stdout;
    assert!(empty.is_empty(), "real backend EOF has no synthetic reply");
    let clean = received(&empty).await;
    assert_eq!(clean.len(), 1, "zero-byte EOF is normal");
    frame_events::assert_closed(&clean, None);
    let (frame, expected) = produced_frame().await;
    assert_truncation(frame.get(..2).expect("real torn native prefix")).await;
    assert_truncation(frame.get(..5).expect("real torn native body")).await;
    valid_then_truncated(&frame, &expected).await;
}

/// Earlier real responses remain ordered when a following native frame is torn.
async fn valid_then_truncated(frame: &[u8], expected: &Value) {
    let bytes = [frame, frame.get(..5).expect("torn second native frame")].concat();
    let events = received(&bytes).await;
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["kind"], "response");
    assert_eq!(events[0]["envelope"], *expected);
    assert_eq!(
        events[1]["kind"], "error",
        "valid traffic cannot hide truncation"
    );
    assert_failure(&events);
}

/// Opening a directory succeeds on Unix, but its first actual read returns EISDIR.
#[cfg(unix)]
#[tokio::test]
async fn native_initial_read_failure_stays_visible() {
    let directory = tempfile::TempDir::new().expect("real directory input");
    let source = directory.path().to_str().expect("UTF-8 fixture path");
    let events = frame_events::journal(&["--receive", source], &[]).await;
    assert_failure(&events);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "error");
}
