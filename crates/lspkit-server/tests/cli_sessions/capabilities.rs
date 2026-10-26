//! Standard and malformed initialize capability handling at the protocol
//! boundary of the real reference server. [LSP-PULL-CAPABILITIES]: the plural
//! `workspace.diagnostics` capability clients actually send decodes once at
//! the shared boundary, and malformed initialize data stays an error.

use std::path::Path;

use super::common::{
    assert_backend_failure, clean_exit, error_of, initialize_session, reference_binary, result_of,
    uri_for, ReferenceClient, StepResult,
};
use lspkit_server::Message;
use serde_json::{json, Value};
use tempfile::TempDir;

/// The full advertisement every accepted initialize must produce.
pub(super) fn assert_advertised(capabilities: &Value) {
    let provider = "/capabilities/diagnosticProvider";
    assert_eq!(
        capabilities.pointer("/capabilities/positionEncoding"),
        Some(&json!("utf-16"))
    );
    assert_eq!(
        capabilities.pointer("/capabilities/textDocumentSync"),
        Some(&json!(1))
    );
    assert_eq!(
        capabilities.pointer(&format!("{provider}/identifier")),
        Some(&json!("diagnostic-reference"))
    );
    assert_eq!(
        capabilities.pointer(&format!("{provider}/interFileDependencies")),
        Some(&json!(true))
    );
    assert_eq!(
        capabilities.pointer(&format!("{provider}/workspaceDiagnostics")),
        Some(&json!(true))
    );
}

/// An accepted initialize response decodes and carries the advertisement.
fn assert_decodes(response: &Message, label: &str) {
    assert!(response.error.is_none(), "{label}");
    assert_advertised(&result_of(response));
}

/// Malformed initialize data must retain the informative protocol error.
fn assert_malformed(response: &Message) {
    match response.error.clone() {
        Some(error) => {
            assert_eq!(error.code, -32602, "malformed initialize");
            assert!(
                !error.message.is_empty(),
                "the decode error stays informative: {error:?}"
            );
        }
        None => assert!(
            response.error.is_some(),
            "malformed initialize answers an error: {response:?}"
        ),
    }
}

/// Issue the four initialize variants in protocol order.
async fn initialize_variants(
    client: &mut ReferenceClient,
) -> StepResult<(Message, Message, Message, Message)> {
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
        .await?;
    let malformed = client
        .request("initialize", json!({ "capabilities": "not-an-object" }))
        .await?;
    let singular = client
        .request(
            "initialize",
            json!({
                "capabilities": { "workspace": { "diagnostic": { "refreshSupport": true } } }
            }),
        )
        .await?;
    let absent = client
        .request("initialize", json!({ "capabilities": {} }))
        .await?;
    Ok((plural, malformed, singular, absent))
}

#[tokio::test]
async fn capability_variants_advertise_identically_and_malformed_stays_an_error() {
    let _binary = reference_binary().expect("real reference CLI producer was built");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    let (plural, malformed, singular, absent) = initialize_variants(&mut client)
        .await
        .expect("initialize variants round trip");
    assert_decodes(&plural, "plural capability decodes");
    assert_malformed(&malformed);
    assert_decodes(&singular, "singular capability decodes");
    assert_eq!(
        result_of(&singular),
        result_of(&plural),
        "plural and singular produce the same advertisement"
    );
    assert_decodes(&absent, "empty capabilities decode");
    assert_eq!(result_of(&absent), result_of(&plural));
    let unknown = client
        .request("workspace/noSuchMethod", json!({}))
        .await
        .expect("unknown method round trip");
    let error = error_of(&unknown).expect("unknown method answers an error");
    assert_eq!(error.code, -32601, "unknown method");

    clean_exit!(client);
}

/// Scan the workspace while its real source stays unreadable.
async fn scan_unreadable(client: &mut ReferenceClient) -> StepResult<(Message, Vec<Value>)> {
    let token = json!("invalid-native-source");
    client
        .request_with_progress(
            "workspace/diagnostic",
            json!({"previousResultIds":[],"partialResultToken":token}),
            &token,
        )
        .await
}

/// Repair the source on disk and pull it again through the watcher.
async fn repaired_pull(client: &mut ReferenceClient, workspace: &Path) -> StepResult<Message> {
    let source = workspace.join("invalid.json");
    std::fs::write(&source, b"{\"fixed\":true}")?;
    let _changed = client.notification("$/referenceWorkspaceChanged").await?;
    client.pull_document(&uri_for(&source)?).await
}

/// The failed native source is an honest scan error with no fabricated partials.
fn assert_scan_failure(response: &Message, partials: &[Value]) {
    assert_backend_failure(response);
    assert!(
        partials.is_empty(),
        "failed source never streams a fabricated clean report"
    );
}

/// A real filesystem repair invalidates the failed memo and recovers normally.
fn assert_repaired(response: &Message) {
    assert!(response.error.is_none());
    let report = result_of(response);
    assert_eq!(report.get("kind"), Some(&json!("full")));
    assert_eq!(report.get("items"), Some(&json!([])));
    assert!(report.get("resultId").is_some_and(Value::is_string));
}

/// A watched workspace with no synced documents still answers pulls of
/// unknown files with an honest backend failure.
#[tokio::test]
async fn pull_of_a_missing_file_is_an_honest_error() {
    let workspace = TempDir::new().expect("tempdir");
    std::fs::write(workspace.path().join("invalid.json"), [0xff])
        .expect("write actual unreadable source");
    let mut client = ReferenceClient::spawn().expect("spawn reference server");
    initialize_session!(client, workspace.path());

    let missing_uri = uri_for(workspace.path().join("missing.json")).expect("fixture uri");
    let missing = client
        .pull_document(&missing_uri)
        .await
        .expect("pull the missing source");
    assert_backend_failure(&missing);
    let (response, partials) = scan_unreadable(&mut client)
        .await
        .expect("scan the unreadable source");
    assert_scan_failure(&response, &partials);
    let repaired = repaired_pull(&mut client, workspace.path())
        .await
        .expect("pull the repaired source");
    assert_repaired(&repaired);
    clean_exit!(client);
}
