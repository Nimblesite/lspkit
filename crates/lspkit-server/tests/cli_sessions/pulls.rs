//! Pull-diagnostic sessions against the real reference server: snapshot
//! identities, memoization with a real backend work counter, version
//! invalidation, and lock-free supersession of in-flight scans.
//! [LSP-PULL-SALSA], [LSP-PULL-CONCURRENCY]

use std::collections::HashSet;

use super::common::held_source::{join_retry, start_retry, HeldSource};
use super::common::{
    assert_partial_document, bulk_workspace, clean_exit, initialize_session, partial_generation,
    report_identity, result_of, uri_for, ReferenceClient, StepResult, SCAN_FILES,
};
use lspkit_server::Message;
use serde_json::{json, Value};
use tempfile::TempDir;

const BROKEN: &str = "{\"unterminated\": ";

/// Exact parser findings are asserted independently of the process harness.
fn assert_broken_report(report: &Value, expected: &serde_json::Error) {
    assert_eq!(report.get("kind"), Some(&json!("full")));
    let items = report.get("items").and_then(Value::as_array);
    assert_eq!(items.map(std::vec::Vec::len), Some(1), "items array");
    let finding = items.and_then(|items| items.first());
    assert_eq!(
        finding.and_then(|finding| finding.get("message")).cloned(),
        Some(json!(expected.to_string()))
    );
    assert_eq!(
        finding
            .and_then(|finding| finding.pointer("/range/start/line"))
            .cloned(),
        Some(json!(expected.line() - 1))
    );
    assert_eq!(
        finding
            .and_then(|finding| finding.pointer("/range/start/character"))
            .cloned(),
        Some(json!(expected.column() - 1))
    );
    assert_eq!(
        finding.and_then(|finding| finding.get("severity")).cloned(),
        Some(json!(1))
    );
    assert_eq!(
        finding.and_then(|finding| finding.get("source")).cloned(),
        Some(json!("serde_json"))
    );
}

/// A repair recomputes one genuine compiler run at the matching snapshot identity.
fn assert_repaired(report: &Value, fields: &(u64, u64, u64), work: &Value) {
    let (revision, version, generation) = *fields;
    assert_eq!(
        report.get("kind"),
        Some(&json!("full")),
        "new version recomputes"
    );
    assert_eq!(
        report.get("items"),
        Some(&json!([])),
        "real parse fix clears diagnostics"
    );
    assert_eq!(version, 2);
    assert_eq!(
        revision, generation,
        "identity couples snapshot and document"
    );
    assert_eq!(
        work.get("analysisRuns"),
        Some(&json!(2)),
        "the new version ran the backend once"
    );
    assert_eq!(
        work.get("revision"),
        Some(&json!(revision)),
        "metadata revision matches the identity revision"
    );
}

/// The matching identity returns unchanged without entering the analyzer again.
fn assert_memoized(response: &Message, result_id: &str, work: &Value) {
    let report = result_of(response);
    assert_eq!(
        report.get("kind"),
        Some(&json!("unchanged")),
        "memoized identity"
    );
    assert_eq!(report.get("resultId"), Some(&json!(result_id)));
    assert_eq!(
        work.get("analysisRuns"),
        Some(&json!(1)),
        "the memoized answer replays without backend work"
    );
}

/// Disk inputs have independent memoized truth, counting only genuine analyses.
fn assert_disk(first: &Message, again: &Message, work: &Value) {
    let report = result_of(first);
    assert_eq!(report.get("kind"), Some(&json!("full")));
    assert_eq!(report.get("items"), Some(&json!([])));
    assert_eq!(result_of(again).get("kind"), Some(&json!("unchanged")));
    assert_eq!(
        work.get("analysisRuns"),
        Some(&json!(3)),
        "each distinct document snapshot costs exactly one run"
    );
}

#[tokio::test]
async fn document_pull_full_reports_memoization_and_version_invalidation() {
    let workspace = TempDir::new().expect("tempdir");
    std::fs::write(workspace.path().join("good.json"), b"{\"answer\": 42}")
        .expect("write good.json");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());

    let broken_uri = uri_for(workspace.path().join("broken.json")).expect("fixture uri");
    client
        .open_document(&broken_uri, 1, BROKEN)
        .await
        .expect("open the broken source");
    let first = client
        .pull_version(&broken_uri, 1, None)
        .await
        .expect("first broken pull");
    let expected =
        serde_json::from_str::<Value>(BROKEN).expect_err("fixture text must fail to parse");
    assert_broken_report(&result_of(&first), &expected);
    let result_id = result_of(&first)
        .get("resultId")
        .and_then(Value::as_str)
        .expect("resultId")
        .to_owned();
    assert!(
        result_id.starts_with("p:"),
        "snapshot identity: {result_id}"
    );
    let work = client.work().await.expect("work metadata");
    assert_eq!(
        work.get("analysisRuns"),
        Some(&json!(1)),
        "exactly one backend run produced the first report"
    );

    let memoized = client
        .pull_version(&broken_uri, 1, Some(&result_id))
        .await
        .expect("memoized pull");
    let work = client.work().await.expect("work metadata");
    assert_memoized(&memoized, &result_id, &work);
    let (fixed, work) = repaired_state(&mut client, &broken_uri)
        .await
        .expect("pull the repaired version");
    let fields = report_identity(&result_of(&fixed)).expect("repaired identity");
    assert_repaired(&result_of(&fixed), &fields, &work);
    let good_uri = uri_for(workspace.path().join("good.json")).expect("fixture uri");
    let (disk, again) = disk_pulls(&mut client, &good_uri)
        .await
        .expect("disk source pulls");
    let work = client.work().await.expect("work metadata");
    assert_disk(&disk, &again, &work);
    clean_exit!(client);
}

/// Edit the repair, pull it, and pair the report with the work metadata.
async fn repaired_state(
    client: &mut ReferenceClient,
    broken_uri: &str,
) -> StepResult<(Message, Value)> {
    client
        .change_document(broken_uri, 2, "{\"ok\": true}")
        .await?;
    let fixed = client.pull_version(broken_uri, 2, None).await?;
    let work = client.work().await?;
    Ok((fixed, work))
}

/// Pull the disk input and repeat with its own identity.
async fn disk_pulls(
    client: &mut ReferenceClient,
    good_uri: &str,
) -> StepResult<(Message, Message)> {
    let disk = client.pull_document(good_uri).await?;
    let disk_id = result_of(&disk)
        .get("resultId")
        .and_then(Value::as_str)
        .ok_or("resultId")?
        .to_owned();
    let again = client.pull_previous(good_uri, &disk_id).await?;
    Ok((disk, again))
}

/// Open a document mid-scan and collect the traffic while the source is held.
async fn open_during_native_scan(
    client: &mut ReferenceClient,
    token: &Value,
    late_uri: &str,
) -> StepResult<(Message, Vec<Value>)> {
    client.open_document(late_uri, 1, "{\"late\": 1}").await?;
    client
        .request_with_progress("reference/work", json!({}), token)
        .await
}

/// The input epoch advances while the actual source reader remains held.
fn assert_open_during_scan(metadata: &Message, work: &Value) {
    assert!(metadata.error.is_none(), "foreground remains responsive");
    assert_eq!(
        work.get("revision"),
        Some(&json!(2)),
        "input invalidation did not wait for analysis"
    );
    assert_eq!(
        work.get("analysisRuns"),
        Some(&json!(SCAN_FILES + 1)),
        "the real scan entered its held source"
    );
}

/// The real stream records both superseded and current snapshot generations.
fn assert_retry_identities(barrier: &Value, partials: &[Value]) {
    let generations: HashSet<_> = std::iter::once(barrier)
        .chain(partials)
        .map(partial_generation)
        .collect();
    assert!(
        generations.contains(&Some(1)) && generations.contains(&Some(2)),
        "both the superseded and retry revisions streamed: {generations:?}"
    );
}

/// The retry covers the new editor input and runs a complete additional pass.
fn assert_retry_contents(partials: &[Value], late_uri: &str) {
    let scanned: HashSet<_> = partials
        .iter()
        .map(|value| {
            value
                .get(0)
                .and_then(|report| report.get("uri"))
                .and_then(Value::as_str)
        })
        .collect();
    assert!(
        scanned.contains(&Some(late_uri)),
        "the retry scans the newly opened document"
    );
    let partial_count = partials.len();
    assert!(
        partial_count > SCAN_FILES,
        "a full retry wave streamed after the supersession: {partial_count}"
    );
}

/// A successful terminal reply contains no duplicate streamed reports.
fn assert_retry_completion(terminal: &Message, work: &Value) {
    assert_eq!(
        result_of(terminal).get("items"),
        Some(&json!([])),
        "with a token, every report streams as a partial"
    );
    let files = u64::try_from(SCAN_FILES).unwrap_or(u64::MAX);
    let runs = work
        .get("analysisRuns")
        .and_then(Value::as_u64)
        .map(|runs| runs > files);
    assert_eq!(
        runs,
        Some(true),
        "supersession recomputed real documents under the new revision"
    );
}

/// An in-flight scan must not block invalidation: opening a document bumps
/// the revision lock-free mid-scan, the superseded pass is discarded, and
/// the retry streams under the new revision. A real native source read is
/// held until the foreground reader consumes the new editor input.
#[tokio::test]
async fn invalidation_supersedes_an_in_flight_scan_without_blocking() {
    let mut fixture = HeldSource::new(SCAN_FILES).expect("held-source fixture");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, fixture.workspace.path());
    let token = json!("supersede-token");
    let id = client.start_scan(&token).await.expect("start the scan");
    let barrier = client
        .first_partial(&id, &token)
        .await
        .expect("first streamed partial");
    assert_partial_document(&barrier);
    let held = fixture.writer().expect("hold the native source read");
    let late_uri = uri_for(fixture.workspace.path().join("opened-late.json")).expect("fixture uri");
    let (metadata, mut early) = open_during_native_scan(&mut client, &token, &late_uri)
        .await
        .expect("open a document during the held scan");
    assert_open_during_scan(&metadata, &result_of(&metadata));
    let retry = held.release(true).expect("arm the native retry writer");
    let retried = client
        .first_partial(&id, &token)
        .await
        .expect("retry partial");
    assert_partial_document(&retried);
    assert_eq!(partial_generation(&retried), Some(2));
    early.push(retried);
    let retry = start_retry(retry).expect("start the retry writer");
    let (terminal, partials) = client
        .collect_stream(&id, &token)
        .await
        .expect("collect the superseded scan");
    early.extend(partials);
    assert_retry_identities(&barrier, &early);
    assert_retry_contents(&early, &late_uri);
    let work = client.work().await.expect("work metadata");
    assert_retry_completion(&terminal, &work);
    join_retry(retry).expect("supply real retry input");
    clean_exit!(client);
}

#[tokio::test]
async fn repeated_queries_share_one_backend_run_until_invalidated() {
    let workspace = bulk_workspace(4).expect("scan workspace fixture");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());

    let first_uri = uri_for(workspace.path().join("scan-0000.json")).expect("fixture uri");
    let first = client.pull_document(&first_uri).await.expect("first query");
    let report = result_of(&first);
    assert_eq!(report.get("kind"), Some(&json!("full")));
    let work = client.work().await.expect("work metadata");
    assert_eq!(
        work.get("analysisRuns"),
        Some(&json!(1)),
        "one backend run behind the first query"
    );

    let second = client
        .pull_document(&first_uri)
        .await
        .expect("repeat query");
    assert_eq!(
        result_of(&second).get("resultId"),
        report.get("resultId"),
        "inspection stays memoized"
    );
    let work = client.work().await.expect("work metadata");
    assert_eq!(
        work.get("analysisRuns"),
        Some(&json!(1)),
        "the repeat query never reaches the backend"
    );
    clean_exit!(client);
}
