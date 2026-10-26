//! End-to-end sessions against the real compiled `diagnostic-reference`
//! server over framed stdio with real files on disk — no fakes, no mocks.
//! Spec-derived assertions per [LSP-PULL-CAPABILITIES], [LSP-PULL-SALSA],
//! [LSP-PULL-CANCELLATION] and [LSP-PULL-WATCH].

use super::capabilities::assert_advertised;
use super::common::{
    assert_backend_failure, error_of, identity, initialize_at, reference_binary, result_of,
    uri_for, ReferenceClient,
};
use serde_json::{json, Value};
use tempfile::TempDir;

/// Each actual scanned source emits one ordered partial before the terminal reply.
fn assert_scan_order(partials: &[Value], expected: &[String]) {
    let scanned: Vec<String> = partials
        .iter()
        .map(|value| value[0]["uri"].as_str().expect("partial uri").to_owned())
        .collect();
    assert_eq!(
        scanned, expected,
        "one ordered partial per scanned document"
    );
}

/// The identity primed through a real document pull is unchanged in the scan.
fn assert_unchanged_partial(partials: &[Value], good_uri: &str) {
    let good_item = partials
        .iter()
        .find(|value| value[0]["uri"] == good_uri)
        .expect("good.json partial");
    assert_eq!(
        good_item[0]["kind"], "unchanged",
        "primed identity answers unchanged inside the scan"
    );
}

/// A real JSON array yields the compiler's structural warning.
fn assert_warning_partial(partials: &[Value]) {
    let plain_item = partials
        .iter()
        .find(|value| {
            value[0]["uri"]
                .as_str()
                .is_some_and(|uri| uri.ends_with("plain.json"))
        })
        .expect("plain.json partial");
    let plain_items = plain_item[0]["items"].as_array().expect("warning items");
    assert_eq!(plain_items.len(), 1);
    assert_eq!(plain_items[0]["severity"], 2);
    assert_eq!(
        plain_items[0]["message"],
        "top-level JSON value must be an object"
    );
}

/// An actual native watcher event advances identity and re-reports the exact error.
fn assert_regressed_report(report: &Value, generation_before: u64) {
    assert_eq!(report["kind"], "full");
    let items = report["items"].as_array().expect("items");
    assert_eq!(
        items.len(),
        1,
        "watcher-driven regression re-reports the error"
    );
    let expected =
        serde_json::from_str::<Value>("{\"broken\": ").expect_err("fixture must fail to parse");
    assert_eq!(items[0]["message"], expected.to_string());
    assert_eq!(items[0]["source"], "serde_json");
    assert_eq!(items[0]["severity"], 1);
    let (_, _, generation_regressed) = identity(report["resultId"].as_str().expect("resultId"));
    assert!(
        generation_regressed > generation_before,
        "watcher invalidation advances the snapshot revision"
    );
}

#[tokio::test]
async fn initialize_boundary_and_clean_exit() {
    let workspace = TempDir::new().expect("tempdir");
    let mut client = ReferenceClient::spawn();

    initialize_at(&mut client, workspace.path()).await;
    let advertised = client
        .request("initialize", json!({ "capabilities": {} }))
        .await;
    assert!(
        result_of(&advertised).get("capabilities").is_some(),
        "capabilities"
    );
    assert_advertised(&result_of(&advertised));

    let unknown = client.request("session/noSuchMethod", json!({})).await;
    assert_eq!(error_of(&unknown).code, -32601, "unknown method");

    let status = client.shutdown_and_exit().await;
    assert!(status.success(), "clean exit, got {status:?}");
    client.read_eof().await;
}

/// The example binary the suites drive must exist on disk and be runnable.
#[test]
fn reference_binary_is_a_real_compiled_program() {
    let binary = reference_binary();
    let metadata = std::fs::metadata(&binary).expect("binary exists on disk");
    assert!(metadata.is_file(), "{binary:?} is a file");
    assert!(metadata.len() > 0, "{binary:?} is non-empty");
}

#[tokio::test]
async fn stdin_eof_ends_the_session_cleanly() {
    let mut client = ReferenceClient::spawn();
    let status = client.close_stdin_and_wait().await;
    assert!(status.success(), "EOF is a clean shutdown, got {status:?}");
    client.read_eof().await;
}

/// Real disk fixtures include clean, warning and subsequently removed sources.
fn scan_workspace() -> TempDir {
    let workspace = TempDir::new().expect("tempdir");
    std::fs::write(workspace.path().join("good.json"), b"{\"answer\": 42}")
        .expect("write good.json");
    std::fs::write(workspace.path().join("plain.json"), b"[1, 2, 3]").expect("write plain.json");
    std::fs::write(workspace.path().join("unread.json"), b"{\"initial\": true}")
        .expect("write unread.json");
    workspace
}

#[tokio::test]
async fn workspace_scan_streams_ordered_partials_and_survives_backend_failures() {
    let workspace = scan_workspace();
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;
    let (good_uri, generation_before) = initial_scan(&mut client, workspace.path()).await;
    assert_removed_source(&mut client, workspace.path()).await;
    assert_restored_source(&mut client, workspace.path()).await;
    assert_regression(&mut client, workspace.path(), &good_uri, generation_before).await;
    assert_native_repair(&mut client, workspace.path(), &good_uri).await;
    client.assert_clean_exit().await;
}

/// Open editor-only input and prime one real disk identity before scanning.
async fn initial_scan(client: &mut ReferenceClient, workspace: &std::path::Path) -> (String, u64) {
    let virtual_uri = uri_for(workspace.join("virtual.json"));
    client
        .open_document(&virtual_uri, 1, "{\"virtual\": true}")
        .await;
    let good_uri = uri_for(workspace.join("good.json"));
    let primed = client.pull_document(&good_uri).await;
    let primed_id = result_of(&primed)["resultId"]
        .as_str()
        .expect("resultId")
        .to_owned();
    let (_, _, generation_before) = identity(&primed_id);
    let (response, partials) = scan_primed(client, &good_uri, &primed_id).await;
    assert_initial_scan(&response, &partials, workspace, &good_uri, &virtual_uri);
    (good_uri, generation_before)
}

/// Scan via the actual standard previousResultIds and partialResultToken fields.
async fn scan_primed(
    client: &mut ReferenceClient,
    good_uri: &str,
    primed_id: &str,
) -> (lspkit_server::Message, Vec<Value>) {
    let token = json!("scan-token");
    client
        .request_with_progress(
            "workspace/diagnostic",
            json!({
                "previousResultIds": [ { "uri": good_uri, "value": primed_id } ],
                "partialResultToken": token
            }),
            &token,
        )
        .await
}

/// The exact original source set, order, memoized report and warning all survive.
fn assert_initial_scan(
    response: &lspkit_server::Message,
    partials: &[Value],
    workspace: &std::path::Path,
    good_uri: &str,
    virtual_uri: &str,
) {
    let mut expected = [
        good_uri.to_owned(),
        uri_for(workspace.join("plain.json")),
        uri_for(workspace.join("unread.json")),
        virtual_uri.to_owned(),
    ];
    expected.sort();
    assert_scan_order(partials, &expected);

    // The streamed partials carry the real per-document reports.
    assert_eq!(
        result_of(response)["items"]
            .as_array()
            .expect("items")
            .len(),
        0
    );
    assert_unchanged_partial(partials, good_uri);
    assert_warning_partial(partials);
}

/// A native removal produces an honest backend error, never fabricated clean truth.
async fn assert_removed_source(client: &mut ReferenceClient, workspace: &std::path::Path) {
    let unread_uri = uri_for(workspace.join("unread.json"));
    std::fs::remove_file(workspace.join("unread.json")).expect("remove unread.json");
    client.notification("$/referenceWorkspaceChanged").await;
    let failure = client.pull_document(&unread_uri).await;
    assert_backend_failure(&failure);
}

/// Native restoration recovers without restarting or inserting editor text.
async fn assert_restored_source(client: &mut ReferenceClient, workspace: &std::path::Path) {
    let unread_uri = uri_for(workspace.join("unread.json"));
    std::fs::write(workspace.join("unread.json"), b"{\"restored\": true}")
        .expect("restore unread.json");
    client.notification("$/referenceWorkspaceChanged").await;
    let recovered = client.pull_document(&unread_uri).await;
    let report = result_of(&recovered);
    assert_eq!(report["kind"], "full", "retry after failure recomputes");
    assert_eq!(report["items"].as_array().expect("items").len(), 0);
}

/// A subsequent native regression yields the actual parser error at a newer epoch.
async fn assert_regression(
    client: &mut ReferenceClient,
    workspace: &std::path::Path,
    good_uri: &str,
    generation_before: u64,
) {
    std::fs::write(workspace.join("good.json"), b"{\"broken\": ").expect("regress good.json");
    client.notification("$/referenceWorkspaceChanged").await;
    let regressed = client.pull_document(good_uri).await;
    let report = result_of(&regressed);
    assert_regressed_report(&report, generation_before);
}

/// A native fix clears the error again through the watcher-driven input revision.
async fn assert_native_repair(
    client: &mut ReferenceClient,
    workspace: &std::path::Path,
    good_uri: &str,
) {
    std::fs::write(workspace.join("good.json"), b"{\"answer\": 43}").expect("fix good.json");
    client.notification("$/referenceWorkspaceChanged").await;
    let repaired = client.pull_document(good_uri).await;
    assert_eq!(
        result_of(&repaired)["items"]
            .as_array()
            .expect("items")
            .len(),
        0,
        "on-disk fix clears diagnostics again"
    );
}
