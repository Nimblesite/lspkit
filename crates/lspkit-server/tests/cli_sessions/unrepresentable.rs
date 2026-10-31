//! Native source paths cannot silently vanish from workspace truth.
//! [LSP-PULL-SALSA] [LSP-PULL-WATCH]

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use super::common::bulk_workspace;
#[cfg(not(target_vendor = "apple"))]
use super::common::{
    clean_exit, error_of, identity, initialize_session, result_of, uri_for, ReferenceClient,
};
#[cfg(not(target_vendor = "apple"))]
use lspkit_server::{JsonRpcError, Message};
#[cfg(not(target_vendor = "apple"))]
use serde_json::{json, Value};

#[cfg(not(target_vendor = "apple"))]
/// Omitting the source would fabricate a successful, incomplete workspace report.
fn assert_scan_failure(response: &Message, error: &JsonRpcError, work: &Value) {
    assert!(
        response.result.is_none(),
        "an incomplete scan cannot claim successful truth"
    );
    assert_eq!(error.code, -32603);
    assert!(
        error
            .message
            .contains("path cannot be expressed as a file URI"),
        "the failure names the unrepresentable path: {error:?}"
    );
    assert_eq!(
        work.get("analysisRuns"),
        Some(&json!(0)),
        "enumeration failed before analysis"
    );
}

#[cfg(not(target_vendor = "apple"))]
/// Native recovery must return the complete remaining source under a newer epoch.
fn assert_scan_recovered(response: &Message, expected_uri: &str, before: u64) {
    assert!(
        response.error.is_none(),
        "native removal restores an enumerable workspace"
    );
    let report = result_of(response);
    match report
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
    {
        Some([remaining]) => assert_remaining(remaining, expected_uri, before),
        _ => assert_eq!(
            report
                .get("items")
                .and_then(Value::as_array)
                .map(std::vec::Vec::len),
            Some(1),
            "the remaining real source is never lost"
        ),
    }
}

#[cfg(not(target_vendor = "apple"))]
/// The one remaining source is full, clean, and at a newer epoch.
fn assert_remaining(remaining: &Value, expected_uri: &str, before: u64) {
    assert_eq!(remaining.get("uri").cloned(), Some(json!(expected_uri)));
    assert_eq!(remaining.get("kind"), Some(&json!("full")));
    assert_eq!(remaining.get("items"), Some(&json!([])));
    let epoch = remaining
        .get("resultId")
        .and_then(Value::as_str)
        .and_then(|result_id| identity(result_id).ok())
        .map(|(_, version, epoch)| (version, epoch > before));
    assert_eq!(
        epoch,
        Some((0, true)),
        "closed disk input invalidated the failed scan"
    );
}

/// `EILSEQ`: Apple filesystems refuse a name that is not valid UTF-8.
#[cfg(target_vendor = "apple")]
const ILLEGAL_BYTE_SEQUENCE: i32 = 92;

/// A native source filename that is not UTF-8, so no file URI can name it.
fn unrepresentable_source(root: &Path) -> PathBuf {
    root.join(OsString::from_vec(b"invalid-\xff.json".to_vec()))
}

/// APFS and HFS+ store names as UTF-8, so on Apple platforms an unrepresentable
/// source can never enter the workspace the scan enumerates.
#[cfg(target_vendor = "apple")]
#[test]
fn apple_filesystems_refuse_an_unrepresentable_source_name() {
    let workspace = bulk_workspace(1).expect("scan workspace fixture");
    let source = unrepresentable_source(workspace.path());
    let error = std::fs::write(&source, b"{\"broken\":")
        .expect_err("an Apple filesystem refuses a non-UTF-8 name");
    assert_eq!(error.raw_os_error(), Some(ILLEGAL_BYTE_SEQUENCE));
    assert!(!source.exists(), "the refused source never exists");
    let names: Vec<OsString> = std::fs::read_dir(workspace.path())
        .expect("list scan workspace")
        .map(|entry| entry.expect("workspace entry").file_name())
        .collect();
    assert_eq!(names, vec![OsString::from("scan-0000.json")]);
}

/// The actual native filename is legal on Linux and cannot be represented by LSP.
#[cfg(not(target_vendor = "apple"))]
#[tokio::test]
async fn unrepresentable_source_is_an_honest_scan_error_and_native_removal_recovers() {
    let workspace = bulk_workspace(1).expect("scan workspace fixture");
    let source = unrepresentable_source(workspace.path());
    std::fs::write(&source, b"{\"broken\":").expect("write actual unrepresentable source");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());
    let work = client.work().await.expect("work metadata");
    let before = work
        .get("revision")
        .and_then(Value::as_u64)
        .expect("revision");

    let failed = client
        .request("workspace/diagnostic", json!({"previousResultIds":[]}))
        .await
        .expect("scan with the unrepresentable source");
    let error = error_of(&failed).expect("failed scan answers an error");
    let work = client.work().await.expect("work metadata");
    assert_scan_failure(&failed, &error, &work);

    std::fs::remove_file(&source).expect("remove unrepresentable source");
    let _changed = client
        .notification("$/referenceWorkspaceChanged")
        .await
        .expect("watcher removal notification");
    let expected_uri = uri_for(workspace.path().join("scan-0000.json")).expect("fixture uri");
    let recovered = client
        .request("workspace/diagnostic", json!({"previousResultIds":[]}))
        .await
        .expect("scan after native removal");
    assert_scan_recovered(&recovered, &expected_uri, before);
    let work = client.work().await.expect("work metadata");
    assert_eq!(work.get("analysisRuns"), Some(&json!(1)));
    clean_exit!(client);
    client.read_eof().await.expect("stdout reaches EOF");
}
