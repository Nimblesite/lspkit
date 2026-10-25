//! Native source paths cannot silently vanish from workspace truth.
//! [LSP-PULL-SALSA] [LSP-PULL-WATCH]

use super::common::{
    bulk_workspace, error_of, identity, initialize_at, result_of, uri_for, ReferenceClient,
};
use serde_json::json;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

/// The actual native filename is legal on Unix and cannot be represented by LSP.
#[tokio::test]
async fn unrepresentable_source_is_an_honest_scan_error_and_native_removal_recovers() {
    let workspace = bulk_workspace(1);
    let source = workspace
        .path()
        .join(OsString::from_vec(b"invalid-\xff.json".to_vec()));
    std::fs::write(&source, b"{\"broken\":").expect("write actual unrepresentable source");
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;
    let before = client.work().await["revision"].as_u64().expect("revision");
    assert_scan_failure(&mut client).await;
    std::fs::remove_file(&source).expect("remove unrepresentable source");
    client.notification("$/referenceWorkspaceChanged").await;
    let expected_uri = uri_for(workspace.path().join("scan-0000.json"));
    assert_scan_recovered(&mut client, &expected_uri, before).await;
    assert_eq!(client.work().await["analysisRuns"], 1);
    client.assert_clean_exit().await;
    client.read_eof().await;
}

/// Omitting the source would fabricate a successful, incomplete workspace report.
async fn assert_scan_failure(client: &mut ReferenceClient) {
    let response = client
        .request("workspace/diagnostic", json!({"previousResultIds":[]}))
        .await;
    assert!(
        response.result.is_none(),
        "an incomplete scan cannot claim successful truth"
    );
    let failure = error_of(&response);
    assert_eq!(failure.code, -32603);
    assert!(failure
        .message
        .contains("path cannot be expressed as a file URI"));
    assert_eq!(
        client.work().await["analysisRuns"],
        0,
        "enumeration failed before analysis"
    );
}

/// Native recovery must return the complete remaining source under a newer epoch.
async fn assert_scan_recovered(client: &mut ReferenceClient, expected_uri: &str, before: u64) {
    let response = client
        .request("workspace/diagnostic", json!({"previousResultIds":[]}))
        .await;
    assert!(
        response.error.is_none(),
        "native removal restores an enumerable workspace"
    );
    let report = result_of(&response);
    let items = report["items"].as_array().expect("workspace reports");
    assert_eq!(items.len(), 1, "the remaining real source is never lost");
    assert_eq!(items[0]["uri"], expected_uri);
    assert_eq!(items[0]["kind"], "full");
    assert_eq!(items[0]["items"].as_array().expect("diagnostics").len(), 0);
    let (_, version, epoch) = identity(items[0]["resultId"].as_str().expect("resultId"));
    assert_eq!(version, 0, "closed disk input");
    assert!(
        epoch > before,
        "actual native observation invalidated the failed scan"
    );
}
