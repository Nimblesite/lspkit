//! Request lifetime at the real server boundary. [LSP-PULL-CANCELLATION]:
//! cancellation is registered before a request is scheduled, `$/cancelRequest`
//! stops in-flight scans, request ids are reusable, `exit` cancels everything
//! outstanding, and a terminal reply closes the partial stream.

use super::common::{
    bulk_workspace, error_of, identity, initialize_at, result_of, uri_for, ReferenceClient,
    SCAN_FILES,
};
use lspkit_server::RequestId;
use serde_json::json;
use tempfile::TempDir;

/// Batched input frames are read in order before spawned work is polled, so
/// a `$/cancelRequest` sent behind a scan request trips the token before its
/// first poll: the terminal error arrives with zero backend work.
#[tokio::test]
async fn cancel_before_the_first_poll_never_runs_the_backend() {
    let workspace = bulk_workspace(4);
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;

    let token = json!("cancel-before-poll");
    let id = client
        .send_cancelled(
            "workspace/diagnostic",
            json!({ "previousResultIds": [], "partialResultToken": token }),
        )
        .await;
    // The cancel frame is already queued behind the scan request before the
    // server reads either one; the current-thread reference runtime reads
    // both before the spawned scan is first polled.
    let (terminal, partials) = client.collect_stream(&id, &token).await;
    assert_cancelled_before(&mut client, &terminal, &partials).await;
    client.assert_clean_exit().await;
}

/// Preserve cancellation, zero streamed reports and zero actual analysis work.
async fn assert_cancelled_before(
    client: &mut ReferenceClient,
    terminal: &lspkit_server::Message,
    partials: &[serde_json::Value],
) {
    let error = error_of(terminal);
    assert_eq!(
        error.code, -32800,
        "cancelled before the first poll: {error:?}"
    );
    assert!(partials.is_empty(), "no partial could stream: {partials:?}");
    assert_eq!(
        client.work().await["analysisRuns"],
        0,
        "no document was ever analyzed for the cancelled scan"
    );
}

/// A cancel arriving mid-scan (after the first streamed partial) stops the
/// scan and answers the standard cancellation error.
#[tokio::test]
async fn cancel_during_a_scan_stops_the_stream_with_the_standard_error() {
    let workspace = bulk_workspace(SCAN_FILES);
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;

    let token = json!("cancel-mid-scan");
    let id = client.start_scan(&token).await;
    // Barrier: the first streamed partial proves the scan is in flight.
    client.first_partial(&id, &token).await;
    client.notify("$/cancelRequest", json!({ "id": id })).await;

    let (terminal, partials) = client.collect_stream(&id, &token).await;
    let error = error_of(&terminal);
    assert_eq!(error.code, -32800, "standard cancellation error: {error:?}");
    assert!(
        !partials.is_empty(),
        "the scan had already streamed documents"
    );
    client.assert_clean_exit().await;
}

/// A request id whose reply already arrived is freely reusable.
#[tokio::test]
async fn completed_request_ids_are_reusable() {
    let workspace = TempDir::new().expect("tempdir");
    let broken_uri = uri_for(workspace.path().join("reused.json"));
    std::fs::write(workspace.path().join("reused.json"), b"{\"ok\": 1}").expect("write fixture");
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;
    let id = RequestId::Number(4242);
    first_reused_report(&mut client, &id, &broken_uri).await;
    second_reused_report(&mut client, &id, &broken_uri).await;
    client.assert_clean_exit().await;
}

/// The initial real reply releases its request slot.
async fn first_reused_report(client: &mut ReferenceClient, id: &RequestId, broken_uri: &str) {
    client
        .send_reusing(
            id,
            "textDocument/diagnostic",
            json!({ "textDocument": { "uri": broken_uri } }),
        )
        .await;
    let first = client.response(id).await;
    assert_eq!(result_of(&first)["kind"], "full");
}

/// A full-text edit and second reply through that same id proves reuse.
async fn second_reused_report(client: &mut ReferenceClient, id: &RequestId, broken_uri: &str) {
    client
        .change_document(broken_uri, 2, "{\"fixed\": true}")
        .await;
    client
        .send_reusing(
            id,
            "textDocument/diagnostic",
            json!({ "textDocument": { "uri": broken_uri, "version": 2 } }),
        )
        .await;
    let second = client.response(id).await;
    let report = result_of(&second);
    assert_eq!(report["kind"], "full");
    assert_eq!(report["items"].as_array().expect("items").len(), 0);
    assert_eq!(
        identity(report["resultId"].as_str().expect("resultId")).1,
        2
    );
}

/// `exit` cancels every in-flight request and the process still exits 0.
#[tokio::test]
async fn exit_cancels_in_flight_work_and_exits_cleanly() {
    let workspace = bulk_workspace(SCAN_FILES);
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;

    client.start_scan(&json!("exit-token")).await;
    client.notify("exit", json!({})).await;
    let status = client.wait_for_exit().await;
    assert!(status.success(), "exit with in-flight work: {status:?}");
    client.read_eof().await;
}

/// Dropping stdin with work in flight is a clean client-driven teardown.
#[tokio::test]
async fn stdin_eof_with_work_in_flight_exits_cleanly() {
    let workspace = bulk_workspace(SCAN_FILES);
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;

    client.start_scan(&json!("eof-token")).await;
    let status = client.close_stdin_and_wait().await;
    assert!(status.success(), "EOF with in-flight work: {status:?}");
    client.read_eof().await;
}

/// Malformed workspace scan parameters answer a terminal protocol error.
#[tokio::test]
async fn malformed_workspace_params_answer_a_terminal_error() {
    let workspace = bulk_workspace(2);
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;

    let response = client
        .request(
            "workspace/diagnostic",
            json!({ "previousResultIds": "not-an-array" }),
        )
        .await;
    let error = error_of(&response);
    assert_eq!(error.code, -32602);
    assert!(
        error.message.contains("workspace"),
        "the decode error names its method: {error:?}"
    );

    let status = client.shutdown_and_exit().await;
    assert!(status.success(), "clean exit, got {status:?}");
}

/// The terminal reply closes the reply stream: after it, no late partial
/// for that token can ever surface.
#[tokio::test]
async fn the_terminal_reply_closes_the_partial_stream() {
    let workspace = bulk_workspace(3);
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;
    let token = json!("gate-token");
    assert_initial_partials(&mut client, &token).await;

    // Any late partial for the closed token would have to precede the
    // metadata reply that proves the server went quiet.
    let late_token = json!("gate-token");
    let (_, late) = client
        .request_with_progress("reference/work", json!({}), &late_token)
        .await;
    assert!(
        late.is_empty(),
        "no partial surfaced after the terminal reply: {late:?}"
    );
    client.assert_clean_exit().await;
}

/// One partial per document and no duplicated terminal reports.
async fn assert_initial_partials(client: &mut ReferenceClient, token: &serde_json::Value) {
    let (response, partials) = client
        .request_with_progress(
            "workspace/diagnostic",
            json!({ "previousResultIds": [], "partialResultToken": token }),
            token,
        )
        .await;
    assert_eq!(partials.len(), 3, "one partial per scanned document");
    assert_eq!(
        result_of(&response)["items"]
            .as_array()
            .expect("items")
            .len(),
        0,
        "the terminal reply carries no duplicated reports"
    );
}
