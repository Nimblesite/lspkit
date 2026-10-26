//! End-to-end sessions against the real compiled `diagnostic-reference`
//! server over framed stdio with real files on disk — no fakes, no mocks.
//! Spec-derived assertions per [LSP-PULL-CAPABILITIES], [LSP-PULL-SALSA],
//! [LSP-PULL-CANCELLATION] and [LSP-PULL-WATCH].

use std::path::Path;

use super::capabilities::assert_advertised;
use super::common::{
    assert_backend_failure, clean_exit, error_of, identity, initialize_session, reference_binary,
    report_identity, result_of, uri_for, ReferenceClient, StepResult,
};
use lspkit_server::Message;
use serde_json::{json, Value};
use tempfile::TempDir;

/// Each actual scanned source emits one ordered partial before the terminal reply.
fn assert_scan_order(partials: &[Value], expected: &[String]) {
    let scanned: Vec<Option<&str>> = partials
        .iter()
        .map(|value| {
            value
                .get(0)
                .and_then(|report| report.get("uri"))
                .and_then(Value::as_str)
        })
        .collect();
    let expected: Vec<Option<&str>> = expected.iter().map(|uri| Some(uri.as_str())).collect();
    assert_eq!(
        scanned, expected,
        "one ordered partial per scanned document"
    );
}

/// The identity primed through a real document pull is unchanged in the scan.
fn assert_unchanged_partial(partials: &[Value], good_uri: &str) {
    let kind = partials
        .iter()
        .filter_map(|value| value.get(0))
        .find(|report| report.get("uri") == Some(&json!(good_uri)))
        .and_then(|report| report.get("kind").cloned());
    assert_eq!(
        kind,
        Some(json!("unchanged")),
        "good.json partial: primed identity answers unchanged inside the scan"
    );
}

/// A real JSON array yields the compiler's structural warning.
fn assert_warning_partial(partials: &[Value]) {
    let warnings = partials
        .iter()
        .filter_map(|value| value.get(0))
        .find(|report| {
            report
                .get("uri")
                .and_then(Value::as_str)
                .is_some_and(|uri| uri.ends_with("plain.json"))
        })
        .and_then(|report| report.get("items").and_then(Value::as_array))
        .map(|items| {
            items
                .iter()
                .map(|item| (item.get("severity").cloned(), item.get("message").cloned()))
                .collect::<Vec<_>>()
        });
    assert_eq!(
        warnings,
        Some(vec![(
            Some(json!(2)),
            Some(json!("top-level JSON value must be an object")),
        )]),
        "plain.json partial warning items"
    );
}

/// An actual native watcher event advances identity and re-reports the exact error.
fn assert_regressed_report(report: &Value, expected: &serde_json::Error, before: u64) {
    assert_eq!(report.get("kind"), Some(&json!("full")));
    let items = report.get("items").and_then(Value::as_array);
    assert_eq!(
        items.map(std::vec::Vec::len),
        Some(1),
        "watcher-driven regression re-reports the error"
    );
    let finding = items.and_then(|items| items.first());
    assert_eq!(
        finding.and_then(|finding| finding.get("message")).cloned(),
        Some(json!(expected.to_string()))
    );
    assert_eq!(
        finding.and_then(|finding| finding.get("source")).cloned(),
        Some(json!("serde_json"))
    );
    assert_eq!(
        finding.and_then(|finding| finding.get("severity")).cloned(),
        Some(json!(1))
    );
    let generation = report_identity(report)
        .ok()
        .map(|(_, _, generation)| generation > before);
    assert_eq!(
        generation,
        Some(true),
        "watcher invalidation advances the snapshot revision"
    );
}

#[tokio::test]
async fn initialize_boundary_and_clean_exit() {
    let workspace = TempDir::new().expect("tempdir");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());
    let advertised = client
        .request("initialize", json!({ "capabilities": {} }))
        .await
        .expect("second initialize round trip");
    assert!(
        result_of(&advertised).get("capabilities").is_some(),
        "capabilities"
    );
    assert_advertised(&result_of(&advertised));

    let unknown = client
        .request("session/noSuchMethod", json!({}))
        .await
        .expect("unknown method round trip");
    let error = error_of(&unknown).expect("unknown method answers an error");
    assert_eq!(error.code, -32601, "unknown method");

    clean_exit!(client);
    client.read_eof().await.expect("stdout reaches EOF");
}

/// The example binary the suites drive must exist on disk and be runnable.
#[test]
fn reference_binary_is_a_real_compiled_program() {
    let binary = reference_binary().expect("real reference CLI producer was built");
    let metadata = std::fs::metadata(&binary).expect("binary exists on disk");
    assert!(metadata.is_file(), "{binary:?} is a file");
    assert!(metadata.len() > 0, "{binary:?} is non-empty");
}

#[tokio::test]
async fn stdin_eof_ends_the_session_cleanly() {
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    let status = client
        .close_stdin_and_wait()
        .await
        .expect("child exits after EOF");
    assert!(status.success(), "EOF is a clean shutdown, got {status:?}");
    client.read_eof().await.expect("stdout reaches EOF");
}

/// Real disk fixtures include clean, warning and subsequently removed sources.
fn scan_workspace() -> StepResult<TempDir> {
    let workspace = TempDir::new()?;
    std::fs::write(workspace.path().join("good.json"), b"{\"answer\": 42}")?;
    std::fs::write(workspace.path().join("plain.json"), b"[1, 2, 3]")?;
    std::fs::write(workspace.path().join("unread.json"), b"{\"initial\": true}")?;
    Ok(workspace)
}

/// Open editor-only input, prime one real disk identity, then scan it.
async fn initial_scan(
    client: &mut ReferenceClient,
    workspace: &Path,
) -> StepResult<(String, u64, Vec<String>, Message, Vec<Value>)> {
    let virtual_uri = uri_for(workspace.join("virtual.json"))?;
    client
        .open_document(&virtual_uri, 1, "{\"virtual\": true}")
        .await?;
    let good_uri = uri_for(workspace.join("good.json"))?;
    let primed = client.pull_document(&good_uri).await?;
    let primed_id = result_of(&primed)
        .get("resultId")
        .and_then(Value::as_str)
        .ok_or("resultId")?
        .to_owned();
    let generation_before = identity(&primed_id).map(|(_, _, generation)| generation)?;
    let token = json!("scan-token");
    let (response, partials) = client
        .request_with_progress(
            "workspace/diagnostic",
            json!({
                "previousResultIds": [ { "uri": good_uri, "value": primed_id } ],
                "partialResultToken": token
            }),
            &token,
        )
        .await?;
    let mut expected = [
        good_uri.clone(),
        uri_for(workspace.join("plain.json"))?,
        uri_for(workspace.join("unread.json"))?,
        virtual_uri,
    ];
    expected.sort();
    Ok((
        good_uri,
        generation_before,
        expected.to_vec(),
        response,
        partials,
    ))
}

/// The exact original source set, order, memoized report and warning all survive.
fn assert_initial_scan(
    response: &Message,
    partials: &[Value],
    good_uri: &str,
    expected: &[String],
) {
    assert_scan_order(partials, expected);
    // The streamed partials carry the real per-document reports.
    assert_eq!(result_of(response).get("items"), Some(&json!([])));
    assert_unchanged_partial(partials, good_uri);
    assert_warning_partial(partials);
}

/// Remove the unread source and pull it again through the watcher.
async fn remove_source(client: &mut ReferenceClient, workspace: &Path) -> StepResult<Message> {
    std::fs::remove_file(workspace.join("unread.json"))?;
    let _changed = client.notification("$/referenceWorkspaceChanged").await?;
    let unread_uri = uri_for(workspace.join("unread.json"))?;
    client.pull_document(&unread_uri).await
}

/// Restore the removed source and pull it again through the watcher.
async fn restore_source(client: &mut ReferenceClient, workspace: &Path) -> StepResult<Message> {
    std::fs::write(workspace.join("unread.json"), b"{\"restored\": true}")?;
    let _changed = client.notification("$/referenceWorkspaceChanged").await?;
    let unread_uri = uri_for(workspace.join("unread.json"))?;
    client.pull_document(&unread_uri).await
}

/// The restored pull recomputes cleanly after the honest failure.
fn assert_restored(response: &Message) {
    let report = result_of(response);
    assert_eq!(
        report.get("kind"),
        Some(&json!("full")),
        "retry after failure recomputes"
    );
    assert_eq!(report.get("items"), Some(&json!([])));
}

/// Regress the good source and pull it again through the watcher.
async fn regress_source(
    client: &mut ReferenceClient,
    workspace: &Path,
    good_uri: &str,
) -> StepResult<Message> {
    std::fs::write(workspace.join("good.json"), b"{\"broken\": ")?;
    let _changed = client.notification("$/referenceWorkspaceChanged").await?;
    client.pull_document(good_uri).await
}

/// Fix the good source and pull it again through the watcher.
async fn repair_source(
    client: &mut ReferenceClient,
    workspace: &Path,
    good_uri: &str,
) -> StepResult<Message> {
    std::fs::write(workspace.join("good.json"), b"{\"answer\": 43}")?;
    let _changed = client.notification("$/referenceWorkspaceChanged").await?;
    client.pull_document(good_uri).await
}

/// A native fix clears the error again through the watcher-driven input revision.
fn assert_native_repair(response: &Message) {
    assert_eq!(
        result_of(response).get("items"),
        Some(&json!([])),
        "on-disk fix clears diagnostics again"
    );
}

#[tokio::test]
async fn workspace_scan_streams_ordered_partials_and_survives_backend_failures() {
    let workspace = scan_workspace().expect("scan workspace fixture");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());
    let (good_uri, generation_before, expected, response, partials) =
        initial_scan(&mut client, workspace.path())
            .await
            .expect("initial primed scan");
    assert_initial_scan(&response, &partials, &good_uri, &expected);
    let failure = remove_source(&mut client, workspace.path())
        .await
        .expect("pull after native removal");
    assert_backend_failure(&failure);
    let restored = restore_source(&mut client, workspace.path())
        .await
        .expect("pull after native restoration");
    assert_restored(&restored);
    let regressed = regress_source(&mut client, workspace.path(), &good_uri)
        .await
        .expect("pull after native regression");
    let expected_error =
        serde_json::from_str::<Value>("{\"broken\": ").expect_err("fixture must fail to parse");
    assert_regressed_report(&result_of(&regressed), &expected_error, generation_before);
    let repaired = repair_source(&mut client, workspace.path(), &good_uri)
        .await
        .expect("pull after the native fix");
    assert_native_repair(&repaired);
    clean_exit!(client);
}
