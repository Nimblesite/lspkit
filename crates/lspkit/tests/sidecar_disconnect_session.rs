//! Rendered native disconnect sessions against real compiled programs.
//! [LSP-PULL-TRANSPORT]: no test invokes the shared reader or a fake backend.

use serde_json::{json, Value};

use common::{at, entry};

#[path = "sidecar_common/mod.rs"]
mod common;
#[path = "../../../test-support/frame_events.rs"]
#[expect(
    clippy::expect_used,
    reason = "shared test-only module is outside Clippy's test-function detection"
)]
mod frame_events;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Inspect an actual captured native stream through the file-input CLI.
async fn received(bytes: &[u8]) -> TestResult<Vec<Value>> {
    let fixture = tempfile::TempDir::new()?;
    let path = fixture.path().join("stream.bin");
    std::fs::write(&path, bytes)?;
    let source = path.to_str().ok_or("capture path is not UTF-8")?;
    Ok(frame_events::journal(&["--receive", source], &[]).await)
}

/// A source file parsed by the actual backend produces the wire fixture.
async fn produced_frame() -> TestResult<(Vec<u8>, Value)> {
    let fixture = tempfile::TempDir::new()?;
    let path = fixture.path().join("valid.json");
    std::fs::write(&path, b"{\"answer\":42}")?;
    let request = json!({"id":7,"method":"analyze","params":{"path":path}});
    let payload = serde_json::to_vec(&request)?;
    let length = u32::try_from(payload.len())?.to_le_bytes();
    let input = [length.as_slice(), payload.as_slice()].concat();
    let output = frame_events::output(&["--backend"], &input).await;
    let expected = json!({"id":7,"result":{"path":path,"diagnostics":[]}});
    Ok((output.stdout, expected))
}

/// A corrupt native stream produces one visible error, then releases both streams.
fn assert_failure(events: &[Value]) {
    let failures: Vec<_> = events
        .iter()
        .filter(|event| at(event, "/kind") == "error")
        .collect();
    assert_eq!(failures.len(), 1, "one terminal native read error");
    let failure = failures.first().copied().unwrap_or(&Value::Null);
    assert_ne!(at(failure, "/message").as_str().unwrap_or_default(), "");
    frame_events::assert_closed(events, None);
}

/// A torn prefix or body never becomes a successful response or clean EOF.
fn assert_truncation(events: &[Value]) {
    assert_failure(events);
    assert_eq!(
        events.len(),
        2,
        "failure followed by terminal ownership release"
    );
    assert_eq!(at(entry(events, 0), "/kind"), "error");
}

/// Earlier real responses remain ordered when a following native frame is torn.
fn assert_valid_then_truncated(events: &[Value], expected: &Value) {
    assert_eq!(events.len(), 3);
    assert_eq!(at(entry(events, 0), "/kind"), "response");
    assert_eq!(at(entry(events, 0), "/envelope"), expected);
    assert_eq!(
        at(entry(events, 1), "/kind"),
        "error",
        "valid traffic cannot hide truncation"
    );
    assert_failure(events);
}

#[tokio::test]
async fn native_disconnects_preserve_prefix_and_payload_failures_after_valid_traffic() {
    let empty = frame_events::output(&["--backend"], &[]).await.stdout;
    assert!(empty.is_empty(), "real backend EOF has no synthetic reply");
    let clean = received(&empty).await.expect("receive clean native EOF");
    assert_eq!(clean.len(), 1, "zero-byte EOF is normal");
    frame_events::assert_closed(&clean, None);
    let (frame, expected) = produced_frame().await.expect("produce native frame");
    assert!(frame.len() > 5, "actual backend emitted a complete frame");
    let prefix = received(frame.get(..2).unwrap_or_default())
        .await
        .expect("receive torn prefix");
    assert_truncation(&prefix);
    let body = received(frame.get(..5).unwrap_or_default())
        .await
        .expect("receive torn body");
    assert_truncation(&body);
    let bytes = [frame.as_slice(), frame.get(..5).unwrap_or_default()].concat();
    let events = received(&bytes)
        .await
        .expect("receive valid then torn frames");
    assert_valid_then_truncated(&events, &expected);
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
    assert_eq!(at(entry(&events, 0), "/kind"), "error");
}
