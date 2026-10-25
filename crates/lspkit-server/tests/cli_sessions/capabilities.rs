//! Standard and malformed initialize capability handling at the protocol
//! boundary of the real reference server. [LSP-PULL-CAPABILITIES]: the plural
//! `workspace.diagnostics` capability clients actually send decodes once at
//! the shared boundary, and malformed initialize data stays an error.

use super::common::{
    assert_backend_failure, error_of, initialize_at, reference_binary, result_of, uri_for,
    ReferenceClient,
};
use serde_json::{json, Value};
use tempfile::TempDir;

/// The full advertisement every accepted initialize must produce.
pub(super) fn assert_advertised(capabilities: &Value) {
    let capabilities = &capabilities["capabilities"];
    assert_eq!(capabilities["positionEncoding"], "utf-16");
    assert_eq!(capabilities["textDocumentSync"], 1);
    let provider = &capabilities["diagnosticProvider"];
    assert_eq!(provider["identifier"], "diagnostic-reference");
    assert_eq!(provider["interFileDependencies"], true);
    assert_eq!(provider["workspaceDiagnostics"], true);
}

/// LSP and VS Code send the plural `workspace.diagnostics` capability.
async fn plural_advertisement(client: &mut ReferenceClient) -> Value {
    let plural = client
        .request(
            "initialize",
            json!({
                "processId": 1,
                "rootUri": Value::Null,
                "capabilities": {
                    "workspace": { "diagnostics": { "refreshSupport": true } }
                }
            }),
        )
        .await;
    assert!(plural.error.is_none(), "plural capability decodes");
    assert_advertised(&result_of(&plural));
    result_of(&plural)
}

/// Malformed initialize data must retain the informative protocol error.
async fn assert_malformed(client: &mut ReferenceClient) {
    let malformed = client
        .request("initialize", json!({ "capabilities": "not-an-object" }))
        .await;
    let error = error_of(&malformed);
    assert_eq!(error.code, -32602, "malformed initialize");
    assert!(
        !error.message.is_empty(),
        "the decode error stays informative: {error:?}"
    );
}

/// The dependency's singular field remains accepted at the boundary.
async fn assert_singular(client: &mut ReferenceClient, plural: &Value) {
    let singular = client
        .request(
            "initialize",
            json!({
                "capabilities": { "workspace": { "diagnostic": { "refreshSupport": true } } }
            }),
        )
        .await;
    assert!(singular.error.is_none(), "singular capability decodes");
    assert_eq!(
        result_of(&singular),
        *plural,
        "plural and singular produce the same advertisement"
    );
}

#[tokio::test]
async fn capability_variants_advertise_identically_and_malformed_stays_an_error() {
    let _binary = reference_binary();
    let mut client = ReferenceClient::spawn();
    let plural = plural_advertisement(&mut client).await;
    assert_malformed(&mut client).await;
    assert_singular(&mut client, &plural).await;
    // Absent capability data still yields the same server advertisement.
    let absent = client
        .request("initialize", json!({ "capabilities": {} }))
        .await;
    assert!(absent.error.is_none(), "empty capabilities decode");
    assert_eq!(result_of(&absent), plural);
    let unknown = client.request("workspace/noSuchMethod", json!({})).await;
    assert_eq!(error_of(&unknown).code, -32601, "unknown method");

    client.assert_clean_exit().await;
}

/// A watched workspace with no synced documents still answers pulls of
/// unknown files with an honest backend failure.
#[tokio::test]
async fn pull_of_a_missing_file_is_an_honest_error() {
    let workspace = TempDir::new().expect("tempdir");
    std::fs::write(workspace.path().join("invalid.json"), [0xff])
        .expect("write actual unreadable source");
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;

    let missing_uri = uri_for(workspace.path().join("missing.json"));
    let missing = client.pull_document(&missing_uri).await;
    assert_backend_failure(&missing);
    assert_workspace_read_failure(&mut client).await;
    assert_native_source_repair(&mut client, workspace.path()).await;

    let status = client.shutdown_and_exit().await;
    assert!(status.success(), "clean exit, got {status:?}");
}

/// An actual non-UTF8 source is a compiler-input error, never a supersession retry.
async fn assert_workspace_read_failure(client: &mut ReferenceClient) {
    let token = json!("invalid-native-source");
    let (response, partials) = client
        .request_with_progress(
            "workspace/diagnostic",
            json!({"previousResultIds":[],"partialResultToken":token}),
            &token,
        )
        .await;
    assert_backend_failure(&response);
    assert!(
        partials.is_empty(),
        "failed source never streams a fabricated clean report"
    );
}

/// A real filesystem repair invalidates the failed memo and recovers normally.
async fn assert_native_source_repair(client: &mut ReferenceClient, workspace: &std::path::Path) {
    let source = workspace.join("invalid.json");
    std::fs::write(&source, b"{\"fixed\":true}").expect("repair actual source");
    client.notification("$/referenceWorkspaceChanged").await;
    let response = client.pull_document(&uri_for(source)).await;
    assert!(response.error.is_none());
    let report = result_of(&response);
    assert_eq!(report["kind"], "full");
    assert_eq!(report["items"], json!([]));
    assert!(report["resultId"].is_string());
}
