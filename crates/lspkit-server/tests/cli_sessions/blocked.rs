//! Actual blocked source I/O tests responsiveness and snapshot truth.
//! [LSP-PULL-SALSA] [LSP-PULL-CONCURRENCY] [LSP-PULL-CANCELLATION]

use super::common::held_source::{release_original, source_session, HeldSource};
use super::common::{error_of, identity, result_of, ReferenceClient};
use serde_json::json;

/// A real completed clean report supplies the client's opaque previous identity.
async fn prime(client: &mut ReferenceClient, fixture: &HeldSource) -> String {
    let response = client.pull_document(&fixture.good_uri).await;
    assert!(response.error.is_none());
    let report = result_of(&response);
    assert_eq!(report["kind"], "full");
    assert_eq!(report["items"].as_array().expect("items").len(), 0);
    report["resultId"]
        .as_str()
        .expect("real result identity")
        .to_owned()
}

/// Unknown/late cancellation cannot cancel another document or block cached truth.
async fn cached_truth(client: &mut ReferenceClient, fixture: &HeldSource, previous: &str) {
    client
        .notify(
            "$/cancelRequest",
            json!({"id":"unknown-native-source-request"}),
        )
        .await;
    let response = client.pull_previous(&fixture.good_uri, previous).await;
    assert!(response.error.is_none());
    let report = result_of(&response);
    assert_eq!(report["kind"], "unchanged");
    assert_eq!(report["resultId"], previous);
    assert_eq!(
        client.work().await["analysisRuns"],
        2,
        "cached truth never waits for or reruns a compiler"
    );
}

/// The real reader processes a current editor overlay while compiler I/O is held.
async fn replace_source(client: &mut ReferenceClient, fixture: &HeldSource, previous: &str) -> u64 {
    client
        .change_document(&fixture.source_uri, 2, "{\"fixed\":true}")
        .await;
    let work = client.work().await;
    let revision = work["revision"].as_u64().expect("current input revision");
    assert!(
        revision > identity(previous).0,
        "input invalidation did not wait for analysis"
    );
    assert_eq!(
        work["analysisRuns"], 2,
        "updating inputs performs no eager analysis"
    );
    revision
}

/// A current pull uses the overlay, never the superseded source or its errors.
async fn current_truth(client: &mut ReferenceClient, fixture: &HeldSource, revision: u64) {
    let response = client.pull_document(&fixture.source_uri).await;
    assert!(response.error.is_none());
    assert_current_report(&result_of(&response), revision);
    assert_eq!(client.work().await["analysisRuns"], 3);
}

/// Current input is clean, versioned correctly and bound to its actual epoch.
fn assert_current_report(report: &serde_json::Value, revision: u64) {
    assert_eq!(report["kind"], "full");
    assert_eq!(report["items"].as_array().expect("items").len(), 0);
    let (actual, version, generation) = identity(report["resultId"].as_str().expect("resultId"));
    assert_eq!(version, 2);
    assert_eq!(actual, generation);
    assert!(actual >= revision);
}

/// The completed old compiler query is rejected rather than published as current.
fn assert_superseded(response: &lspkit_server::Message) {
    assert!(
        response.result.is_none(),
        "a superseded compiler result is never successful"
    );
    let failure = error_of(response);
    assert_eq!(failure.code, -32603);
    assert!(failure.message.contains("superseded"));
}

#[tokio::test]
async fn cached_pulls_remain_responsive_during_native_source_reads() {
    let (mut fixture, mut client) = source_session(1).await;
    let previous = prime(&mut client, &fixture).await;
    let pending = client
        .send(
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":fixture.source_uri}}),
        )
        .await;
    let writer = fixture.writer();
    cached_truth(&mut client, &fixture, &previous).await;
    let revision = replace_source(&mut client, &fixture, &previous).await;
    release_original(writer);
    let superseded = client.response(&pending).await;
    assert_superseded(&superseded);
    current_truth(&mut client, &fixture, revision).await;
    assert!(client.shutdown_and_exit().await.success());
    client.read_eof().await;
}

/// Cancellation is answered while real compiler I/O remains held by the client.
#[tokio::test]
async fn cancelled_native_scan_rejects_late_compiler_partials() {
    let (mut fixture, mut client) = source_session(1).await;
    let previous = prime(&mut client, &fixture).await;
    let token = json!("cancel-held-native-source");
    let id = client.start_scan(&token).await;
    let writer = fixture.writer();
    assert_cancelled_scan(&mut client, &id, &token, &fixture).await;
    cached_truth(&mut client, &fixture, &previous).await;
    let revision = replace_source(&mut client, &fixture, &previous).await;
    release_original(writer);
    assert_no_late_partial(&mut client, &fixture, &token, revision).await;
    client.assert_clean_exit().await;
    client.read_eof().await;
}

/// Already streamed truth remains intact, with one standard terminal cancellation.
async fn assert_cancelled_scan(
    client: &mut ReferenceClient,
    id: &lspkit_server::RequestId,
    token: &serde_json::Value,
    fixture: &HeldSource,
) {
    client.notify("$/cancelRequest", json!({"id":id})).await;
    let (terminal, partials) = client.collect_stream(id, token).await;
    assert!(
        terminal.result.is_none(),
        "cancellation cannot claim success"
    );
    assert_eq!(error_of(&terminal).code, -32800);
    assert_eq!(partials.len(), 1, "only the completed source was streamed");
    assert_eq!(partials[0][0]["uri"], fixture.good_uri);
    assert_eq!(
        partials[0][0]["items"]
            .as_array()
            .expect("clean source")
            .len(),
        0
    );
    assert_eq!(client.work().await["analysisRuns"], 2);
}

/// The next uncached query is a real compiler-completion barrier for the old work.
async fn assert_no_late_partial(
    client: &mut ReferenceClient,
    fixture: &HeldSource,
    token: &serde_json::Value,
    revision: u64,
) {
    let (response, late) = client
        .request_with_progress(
            "textDocument/diagnostic",
            json!({
                "textDocument":{"uri":fixture.source_uri,"version":2}
            }),
            token,
        )
        .await;
    assert!(response.error.is_none());
    assert!(
        late.is_empty(),
        "the closed stream rejects late compiler output: {late:?}"
    );
    assert_current_report(&result_of(&response), revision);
    assert_eq!(client.work().await["analysisRuns"], 3);
}

/// A real edit supersedes an actually blocked backend, then the scan retries.
#[tokio::test]
async fn invalidated_native_scan_retries_current_overlay() {
    let (mut fixture, mut client) = source_session(1).await;
    let previous = prime(&mut client, &fixture).await;
    let token = json!("invalidated-native-source");
    let id = client.start_scan(&token).await;
    let barrier = client.first_partial(&id, &token).await;
    let writer = fixture.writer();
    let revision = replace_source(&mut client, &fixture, &previous).await;
    release_original(writer);
    let (response, mut partials) = client.collect_stream(&id, &token).await;
    partials.insert(0, barrier);
    assert_retried_reports(&response, &partials, &fixture, revision);
    assert_eq!(client.work().await["analysisRuns"], 4);
    client.assert_clean_exit().await;
    client.read_eof().await;
}

/// The stale backend never streams, and a complete new wave replaces it.
fn assert_retried_reports(
    response: &lspkit_server::Message,
    partials: &[serde_json::Value],
    fixture: &HeldSource,
    revision: u64,
) {
    let generation = |partial: &serde_json::Value| {
        identity(partial[0]["resultId"].as_str().expect("stream identity")).2
    };
    assert!(
        response.error.is_none(),
        "superseded scans retry: {response:?}"
    );
    assert_eq!(result_of(response)["items"], json!([]));
    assert_eq!(partials.len(), 3, "one old and two current reports");
    assert_eq!(partials[0][0]["uri"], fixture.good_uri);
    assert_eq!(generation(&partials[0]), 1);
    assert_eq!(partials[1][0]["uri"], fixture.good_uri);
    assert_eq!(generation(&partials[1]), revision);
    assert_eq!(partials[1][0]["items"], json!([]));
    assert_eq!(partials[2][0]["uri"], fixture.source_uri);
    assert_current_report(&partials[2][0], revision);
}
