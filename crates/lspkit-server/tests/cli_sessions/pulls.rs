//! Pull-diagnostic sessions against the real reference server: snapshot
//! identities, memoization with a real backend work counter, version
//! invalidation, and lock-free supersession of in-flight scans.
//! [LSP-PULL-SALSA], [LSP-PULL-CONCURRENCY]

use super::common::held_source::{join_retry, start_retry, HeldSource};
use super::common::{
    bulk_workspace, identity, initialize_at, result_of, uri_for, ReferenceClient, SCAN_FILES,
};
use serde_json::{json, Value};
use tempfile::TempDir;

const BROKEN: &str = "{\"unterminated\": ";

/// Exact parser findings are asserted independently of the process harness.
fn assert_broken_report(report: &Value) {
    assert_eq!(report["kind"], "full");
    let expected =
        serde_json::from_str::<Value>(BROKEN).expect_err("fixture text must fail to parse");
    let items = report["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["message"], expected.to_string());
    assert_eq!(items[0]["range"]["start"]["line"], expected.line() - 1);
    assert_eq!(
        items[0]["range"]["start"]["character"],
        expected.column() - 1
    );
    assert_eq!(items[0]["severity"], 1);
    assert_eq!(items[0]["source"], "serde_json");
}

/// A repair recomputes one genuine compiler run at the matching snapshot identity.
async fn assert_repaired_snapshot(client: &mut ReferenceClient, report: &Value) {
    let revision = assert_repaired_identity(report);
    let work = client.work().await;
    assert_eq!(
        work["analysisRuns"], 2,
        "the new version ran the backend once"
    );
    assert_eq!(
        work["revision"], revision,
        "metadata revision matches the identity revision"
    );
}

/// The repaired result couples the synchronized version to its revision.
fn assert_repaired_identity(report: &Value) -> u64 {
    assert_eq!(report["kind"], "full", "new version recomputes");
    assert_eq!(
        report["items"].as_array().expect("items").len(),
        0,
        "real parse fix clears diagnostics"
    );
    let (revision, version, generation) = identity(report["resultId"].as_str().expect("resultId"));
    assert_eq!(version, 2);
    assert_eq!(
        revision, generation,
        "identity couples snapshot and document"
    );
    revision
}

#[tokio::test]
async fn document_pull_full_reports_memoization_and_version_invalidation() {
    let workspace = TempDir::new().expect("tempdir");
    std::fs::write(workspace.path().join("good.json"), b"{\"answer\": 42}")
        .expect("write good.json");
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;

    let broken_uri = uri_for(workspace.path().join("broken.json"));
    client.open_document(&broken_uri, 1, BROKEN).await;
    let result_id = first_broken_report(&mut client, &broken_uri).await;
    assert_memoized_report(&mut client, &broken_uri, &result_id).await;
    client
        .change_document(&broken_uri, 2, "{\"ok\": true}")
        .await;
    let fixed = client.pull_version(&broken_uri, 2, None).await;
    assert_repaired_snapshot(&mut client, &result_of(&fixed)).await;
    let good_uri = uri_for(workspace.path().join("good.json"));
    assert_disk_report(&mut client, &good_uri).await;
    client.assert_clean_exit().await;
}

/// The first parser report must be exact and cost exactly one analysis.
async fn first_broken_report(client: &mut ReferenceClient, broken_uri: &str) -> String {
    let first = client.pull_version(broken_uri, 1, None).await;
    let report = result_of(&first);
    assert_broken_report(&report);
    let result_id = report["resultId"].as_str().expect("resultId").to_owned();
    assert!(
        result_id.starts_with("p:"),
        "snapshot identity: {result_id}"
    );
    assert_eq!(
        client.work().await["analysisRuns"],
        1,
        "exactly one backend run produced the first report"
    );
    result_id
}

/// The matching identity returns unchanged without entering the analyzer again.
async fn assert_memoized_report(client: &mut ReferenceClient, broken_uri: &str, result_id: &str) {
    let unchanged = client.pull_version(broken_uri, 1, Some(result_id)).await;
    let report = result_of(&unchanged);
    assert_eq!(report["kind"], "unchanged", "memoized identity");
    assert_eq!(report["resultId"], result_id);
    assert_eq!(
        client.work().await["analysisRuns"],
        1,
        "the memoized answer replays without backend work"
    );
}

/// Disk inputs have independent memoized truth, counting only genuine analyses.
async fn assert_disk_report(client: &mut ReferenceClient, good_uri: &str) {
    let disk = client.pull_document(good_uri).await;
    let report = result_of(&disk);
    assert_eq!(report["kind"], "full");
    assert_eq!(report["items"].as_array().expect("items").len(), 0);
    let disk_id = report["resultId"].as_str().expect("resultId").to_owned();
    let disk_again = client.pull_previous(good_uri, &disk_id).await;
    assert_eq!(result_of(&disk_again)["kind"], "unchanged");
    assert_eq!(
        client.work().await["analysisRuns"],
        3,
        "each distinct document snapshot costs exactly one run"
    );
}

/// An in-flight scan must not block invalidation: opening a document bumps
/// the revision lock-free mid-scan, the superseded pass is discarded, and
/// the retry streams under the new revision. A real native source read is
/// held until the foreground reader consumes the new editor input.
#[tokio::test]
async fn invalidation_supersedes_an_in_flight_scan_without_blocking() {
    let mut fixture = HeldSource::new(SCAN_FILES);
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, fixture.workspace.path()).await;
    let token = json!("supersede-token");
    let id = client.start_scan(&token).await;
    let barrier = client.first_partial(&id, &token).await;
    let held = fixture.writer();
    let late_uri = uri_for(fixture.workspace.path().join("opened-late.json"));
    let mut early = open_during_native_scan(&mut client, &token, &late_uri).await;
    let retry = held.release(true);
    let retried = client.first_partial(&id, &token).await;
    assert_eq!(
        identity(retried[0]["resultId"].as_str().expect("retry identity")).2,
        2
    );
    early.push(retried);
    let retry = start_retry(retry);
    let (terminal, partials) = client.collect_stream(&id, &token).await;
    early.extend(partials);
    assert_retry_identities(&barrier, &early);
    assert_retry_contents(&early, &late_uri);
    assert_retry_completion(&mut client, &terminal).await;
    join_retry(retry);
    client.assert_clean_exit().await;
}

/// The input epoch advances while the actual source reader remains held.
async fn open_during_native_scan(
    client: &mut ReferenceClient,
    token: &Value,
    late_uri: &str,
) -> Vec<Value> {
    client.open_document(late_uri, 1, "{\"late\": 1}").await;
    let (metadata, partials) = client
        .request_with_progress("reference/work", json!({}), token)
        .await;
    assert!(metadata.error.is_none(), "foreground remains responsive");
    let work = result_of(&metadata);
    assert_eq!(
        work["revision"], 2,
        "input invalidation did not wait for analysis"
    );
    assert_eq!(
        work["analysisRuns"],
        SCAN_FILES + 1,
        "the real scan entered its held source"
    );
    partials
}

/// A successful terminal reply contains no duplicate streamed reports.
async fn assert_retry_completion(client: &mut ReferenceClient, terminal: &lspkit_server::Message) {
    assert_eq!(
        result_of(terminal)["items"]
            .as_array()
            .expect("items")
            .len(),
        0,
        "with a token, every report streams as a partial"
    );
    let work = client.work().await;
    let runs = work["analysisRuns"].as_u64().expect("work metadata");
    assert!(
        runs > u64::try_from(SCAN_FILES).expect("fits u64"),
        "supersession recomputed real documents under the new revision: {runs}"
    );
}

/// The real stream records both superseded and current snapshot generations.
fn assert_retry_identities(barrier: &Value, partials: &[Value]) {
    let generations: std::collections::HashSet<_> = std::iter::once(barrier)
        .chain(partials)
        .map(|value| identity(value[0]["resultId"].as_str().expect("identity")).2)
        .collect();
    assert!(
        generations.contains(&1) && generations.contains(&2),
        "both the superseded and retry revisions streamed: {generations:?}"
    );
}

/// The retry covers the new editor input and runs a complete additional pass.
fn assert_retry_contents(partials: &[Value], late_uri: &str) {
    let scanned: std::collections::HashSet<_> = partials
        .iter()
        .map(|value| value[0]["uri"].as_str().expect("uri"))
        .collect();
    assert!(
        scanned.contains(late_uri),
        "the retry scans the newly opened document"
    );
    let partial_count = partials.len();
    assert!(
        partial_count > SCAN_FILES,
        "a full retry wave streamed after the supersession: {partial_count}"
    );
}

#[tokio::test]
async fn repeated_queries_share_one_backend_run_until_invalidated() {
    let workspace = bulk_workspace(4);
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;

    let first_uri = uri_for(workspace.path().join("scan-0000.json"));
    let first = client.pull_document(&first_uri).await;
    let report = result_of(&first);
    assert_eq!(report["kind"], "full");
    assert_eq!(
        client.work().await["analysisRuns"],
        1,
        "one backend run behind the first query"
    );

    assert_repeat_query(&mut client, &first_uri, &report).await;
    client.assert_clean_exit().await;
}

/// A repeat without a previous identity still reuses the same actual parse.
async fn assert_repeat_query(client: &mut ReferenceClient, first_uri: &str, report: &Value) {
    let second = client.pull_document(first_uri).await;
    assert_eq!(
        result_of(&second)["resultId"],
        report["resultId"],
        "inspection stays memoized"
    );
    assert_eq!(
        client.work().await["analysisRuns"],
        1,
        "the repeat query never reaches the backend"
    );
}
