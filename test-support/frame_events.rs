//! Real compiled process input/output and rendered reader assertions.

use serde_json::Value;
use std::process::Output;
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;

#[path = "reference_program.rs"]
mod reference_program;

/// Send native input and wait for the actual program's natural exit.
pub async fn output(args: &[&str], input: &[u8]) -> Output {
    let mut child = reference_program::spawn_reference(args).expect("spawn real reference CLI");
    child
        .stdin
        .take()
        .expect("CLI stdin")
        .write_all(input)
        .await
        .expect("write native CLI input");
    let output = timeout(reference_program::READ_GUARD, child.wait_with_output())
        .await
        .expect("native consumer remains responsive")
        .expect("native program exits");
    assert!(
        output.status.success(),
        "native consumer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Decode only the CLI's rendered events; no test calls the shared reader API.
pub async fn journal(args: &[&str], input: &[u8]) -> Vec<Value> {
    String::from_utf8(output(args, input).await.stdout)
        .expect("UTF-8 journal")
        .lines()
        .map(|line| serde_json::from_str(line).expect("structured rendered event"))
        .collect()
}

/// Both independent streams and their reader terminate once after all traffic.
pub fn assert_closed(events: &[Value], backend_success: Option<bool>) {
    let closed = events.last().expect("terminal rendered event");
    assert_eq!(closed["kind"], "closed");
    assert_eq!(closed["responses_closed"], true);
    assert_eq!(closed["notifications_closed"], true);
    assert_eq!(closed["reader_ended"], true);
    assert_eq!(
        closed["backend_success"],
        serde_json::to_value(backend_success).expect("backend status")
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["kind"] == "closed")
            .count(),
        1
    );
}
