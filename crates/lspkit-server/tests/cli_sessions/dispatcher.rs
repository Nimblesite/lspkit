//! Request lifetime at the real server boundary. [LSP-PULL-CANCELLATION]:
//! cancellation is registered before a request is scheduled, `$/cancelRequest`
//! stops in-flight scans, request ids are reusable, `exit` cancels everything
//! outstanding, and a terminal reply closes the partial stream.

use super::common::held_source::{release_original, HeldSource};
use super::common::{
    assert_partial_document, bulk_workspace, clean_exit, error_of, identity, initialize_session,
    result_of, uri_for, ReferenceClient, StepResult, SCAN_FILES,
};
use lspkit_server::{JsonRpcError, Message, RequestId};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use tempfile::TempDir;

/// Preserve cancellation, zero streamed reports and zero actual analysis work.
fn assert_cancelled_before(error: &JsonRpcError, work: &Value, partials: &[Value]) {
    assert_eq!(
        error.code, -32800,
        "cancelled before the first poll: {error:?}"
    );
    assert!(partials.is_empty(), "no partial could stream: {partials:?}");
    assert_eq!(
        work.get("analysisRuns"),
        Some(&json!(0)),
        "no document was ever analyzed for the cancelled scan"
    );
}

/// Batched input frames are read in order before spawned work is polled, so
/// a `$/cancelRequest` sent behind a scan request trips the token before its
/// first poll: the terminal error arrives with zero backend work.
#[tokio::test]
async fn cancel_before_the_first_poll_never_runs_the_backend() {
    let workspace = bulk_workspace(4).expect("scan workspace fixture");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());

    let token = json!("cancel-before-poll");
    let id = client
        .send_cancelled(
            "workspace/diagnostic",
            json!({ "previousResultIds": [], "partialResultToken": token }),
        )
        .await
        .expect("send the scan with its cancellation");
    // The cancel frame is already queued behind the scan request before the
    // server reads either one; the current-thread reference runtime reads
    // both before the spawned scan is first polled.
    let (terminal, partials) = client
        .collect_stream(&id, &token)
        .await
        .expect("collect the cancelled scan");
    let work = client.work().await.expect("work metadata");
    let error = error_of(&terminal).expect("cancelled scan answers an error");
    assert_cancelled_before(&error, &work, &partials);
    clean_exit!(client);
}

/// The document a streamed partial reports.
fn partial_uri(partial: &Value) -> Option<&str> {
    partial
        .get(0)
        .and_then(|report| report.get("uri"))
        .and_then(Value::as_str)
}

/// The uris of the scan documents, which all sort before the held one.
fn scan_uris(fixture: &HeldSource) -> StepResult<BTreeSet<String>> {
    (0..SCAN_FILES)
        .map(|index| {
            uri_for(
                fixture
                    .workspace
                    .path()
                    .join(format!("scan-{index:04}.json")),
            )
        })
        .collect()
}

/// Every scanned document before the held one streamed exactly once, and the
/// held one never did: the cancel stopped the scan where it was held.
fn assert_streamed_up_to_the_hold(
    fixture: &HeldSource,
    expected: &BTreeSet<String>,
    barrier: &Value,
    partials: &[Value],
) {
    let streamed: Vec<&Value> = std::iter::once(barrier).chain(partials).collect();
    for partial in &streamed {
        assert_partial_document(partial);
    }
    let uris: Vec<&str> = streamed
        .iter()
        .filter_map(|partial| partial_uri(partial))
        .collect();
    let unique: BTreeSet<String> = uris.iter().map(|uri| (*uri).to_owned()).collect();
    assert_eq!(
        uris.len(),
        SCAN_FILES,
        "one partial per document before the hold: {uris:?}"
    );
    assert_eq!(
        &unique, expected,
        "exactly the documents before the hold streamed"
    );
    assert!(
        !unique.contains(&fixture.source_uri),
        "the held document never streamed"
    );
}

/// A cancel arriving mid-scan stops the scan and answers the standard
/// cancellation error. The scan is held on the native read of its last
/// document, so the cancel lands mid-scan by construction, never by timing: a
/// cancel racing the first partial left nothing more streamed some runs.
#[tokio::test]
async fn cancel_during_a_scan_stops_the_stream_with_the_standard_error() {
    let mut fixture = HeldSource::new(SCAN_FILES).expect("held-source fixture");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, fixture.workspace.path());

    let token = json!("cancel-mid-scan");
    let id = client.start_scan(&token).await.expect("start the mid-scan");
    // Barrier: the first streamed partial proves the scan is in flight.
    let barrier = client
        .first_partial(&id, &token)
        .await
        .expect("first streamed partial");
    assert_partial_document(&barrier);
    // Returns once the scan waits on its last document, which sorts after them all.
    let held = fixture
        .writer()
        .expect("hold the scan on its last document");
    client
        .notify("$/cancelRequest", json!({ "id": id }))
        .await
        .expect("cancel the in-flight scan");

    let (terminal, partials) = client
        .collect_stream(&id, &token)
        .await
        .expect("collect the cancelled scan");
    let error = error_of(&terminal).expect("cancelled scan answers an error");
    assert_eq!(error.code, -32800, "standard cancellation error: {error:?}");
    let expected = scan_uris(&fixture).expect("scan document uris");
    assert_streamed_up_to_the_hold(&fixture, &expected, &barrier, &partials);
    release_original(held).expect("release the held read");
    clean_exit!(client);
}

/// Standard pull params for `uri`, optionally pinning a version.
fn pull_params(uri: &str, version: Option<i32>) -> Value {
    match version {
        Some(version) => json!({ "textDocument": { "uri": uri, "version": version } }),
        None => json!({ "textDocument": { "uri": uri } }),
    }
}

/// Send a document pull through an explicit id and await its reply.
async fn reused_report(
    client: &mut ReferenceClient,
    id: &RequestId,
    params: Value,
) -> StepResult<Message> {
    client
        .send_reusing(id, "textDocument/diagnostic", params)
        .await?;
    client.response(id).await
}

/// A request id whose reply already arrived is freely reusable.
#[tokio::test]
async fn completed_request_ids_are_reusable() {
    let workspace = TempDir::new().expect("tempdir");
    std::fs::write(workspace.path().join("reused.json"), b"{\"ok\": 1}").expect("write fixture");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());
    let reused_uri = uri_for(workspace.path().join("reused.json")).expect("fixture uri");
    let id = RequestId::Number(4242);

    let first = reused_report(&mut client, &id, pull_params(&reused_uri, None))
        .await
        .expect("first reply through the reused id");
    assert_eq!(result_of(&first).get("kind"), Some(&json!("full")));

    client
        .change_document(&reused_uri, 2, "{\"fixed\": true}")
        .await
        .expect("full-text edit");
    let second = reused_report(&mut client, &id, pull_params(&reused_uri, Some(2)))
        .await
        .expect("second reply through the same id");
    let report = result_of(&second);
    assert_eq!(report.get("kind"), Some(&json!("full")));
    assert_eq!(report.get("items"), Some(&json!([])));
    let result_id = report
        .get("resultId")
        .and_then(Value::as_str)
        .expect("resultId");
    assert_eq!(identity(result_id).expect("identity fields").1, 2);
    clean_exit!(client);
}

/// `exit` cancels every in-flight request and the process still exits 0.
#[tokio::test]
async fn exit_cancels_in_flight_work_and_exits_cleanly() {
    let workspace = bulk_workspace(SCAN_FILES).expect("scan workspace fixture");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());

    let _scan = client
        .start_scan(&json!("exit-token"))
        .await
        .expect("start a scan before exit");
    client.notify("exit", json!({})).await.expect("send exit");
    let status = client.wait_for_exit().await.expect("child exits");
    assert!(status.success(), "exit with in-flight work: {status:?}");
    client.read_eof().await.expect("stdout reaches EOF");
}

/// Dropping stdin with work in flight is a clean client-driven teardown.
#[tokio::test]
async fn stdin_eof_with_work_in_flight_exits_cleanly() {
    let workspace = bulk_workspace(SCAN_FILES).expect("scan workspace fixture");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());

    let _scan = client
        .start_scan(&json!("eof-token"))
        .await
        .expect("start a scan before EOF");
    let status = client
        .close_stdin_and_wait()
        .await
        .expect("child exits after EOF");
    assert!(status.success(), "EOF with in-flight work: {status:?}");
    client.read_eof().await.expect("stdout reaches EOF");
}

/// Malformed workspace scan parameters answer a terminal protocol error.
#[tokio::test]
async fn malformed_workspace_params_answer_a_terminal_error() {
    let workspace = bulk_workspace(2).expect("scan workspace fixture");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());

    let response = client
        .request(
            "workspace/diagnostic",
            json!({ "previousResultIds": "not-an-array" }),
        )
        .await
        .expect("malformed scan round trip");
    let error = error_of(&response).expect("malformed scan answers an error");
    assert_eq!(error.code, -32602);
    assert!(
        error.message.contains("workspace"),
        "the decode error names its method: {error:?}"
    );
    clean_exit!(client);
}

/// The terminal reply closes the reply stream: after it, no late partial
/// for that token can ever surface.
#[tokio::test]
async fn the_terminal_reply_closes_the_partial_stream() {
    let workspace = bulk_workspace(3).expect("scan workspace fixture");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());
    let token = json!("gate-token");
    let (response, partials) = client
        .request_with_progress(
            "workspace/diagnostic",
            json!({ "previousResultIds": [], "partialResultToken": token }),
            &token,
        )
        .await
        .expect("initial scan round trip");
    assert_initial_partials(&response, &partials);

    // Any late partial for the closed token would have to precede the
    // metadata reply that proves the server went quiet.
    let late_token = json!("gate-token");
    let (_, late) = client
        .request_with_progress("reference/work", json!({}), &late_token)
        .await
        .expect("work metadata round trip");
    assert!(
        late.is_empty(),
        "no partial surfaced after the terminal reply: {late:?}"
    );
    clean_exit!(client);
}

/// One partial per document and no duplicated terminal reports.
fn assert_initial_partials(response: &Message, partials: &[Value]) {
    assert_eq!(partials.len(), 3, "one partial per scanned document");
    assert_eq!(
        result_of(response).get("items"),
        Some(&json!([])),
        "the terminal reply carries no duplicated reports"
    );
}
