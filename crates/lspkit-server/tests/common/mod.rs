//! Shared harness for the real `diagnostic-reference` LSP sessions.
//!
//! Every helper here drives the real compiled example binary as an OS child
//! over framed stdio with real files on disk — no fakes, no mocks. Fallible
//! steps return [`StepResult`] so `#[test]` callers `expect` on them by step
//! name; assertion helpers take already-extracted values and stay panic-free
//! by comparing whole values.

use std::path::{Path, PathBuf};

use lspkit_server::{path_to_uri, JsonRpcError, Message};
use serde_json::{json, Value};
use tempfile::TempDir;

pub mod client;
pub mod held_source;
#[path = "../../../../test-support/reference_program.rs"]
mod reference_program;

pub use client::ReferenceClient;
pub use reference_program::READ_GUARD;

/// A failed harness step: the text names the step and keeps the source error.
pub type StepError = Box<dyn std::error::Error>;
/// Outcome of a fallible harness step; tests `expect` on it by step name.
pub type StepResult<T> = Result<T, StepError>;

/// Files in a bulk workspace fixture; concurrency uses native pipe barriers.
pub const SCAN_FILES: usize = 64;

/// Initialize a session rooted at `root`, asserting the pull advertisement
/// every real client must see. Expands inside the calling `#[test]`.
macro_rules! initialize_session {
    ($client:expr, $root:expr) => {{
        let advertised = crate::common::initialize_at(&mut $client, $root)
            .await
            .expect("session initializes");
        crate::common::assert_pull_advertised(&advertised);
    }};
}
pub(crate) use initialize_session;

/// One initialized scan workspace plus its real framed client. Expands inside
/// the calling `#[test]`, preserving the per-session advertisement assertion.
macro_rules! workspace_session {
    ($count:expr) => {{
        let workspace = crate::common::bulk_workspace($count).expect("scan workspace fixture");
        let mut client = crate::common::ReferenceClient::spawn().expect("spawn reference server");
        crate::common::initialize_session!(client, workspace.path());
        (workspace, client)
    }};
}
pub(crate) use workspace_session;

/// Shut the session down, `exit`, and assert the child's clean exit status.
/// Expands inside the calling `#[test]`.
macro_rules! clean_exit {
    ($client:expr) => {{
        let status = crate::common::ReferenceClient::shutdown_and_exit(&mut $client)
            .await
            .expect("shutdown and exit");
        assert!(status.success(), "clean exit, got {status:?}");
    }};
}
pub(crate) use clean_exit;

/// Pull diagnostics must be advertised for the watched root of every session.
pub fn assert_pull_advertised(advertised: &Value) {
    assert_eq!(
        advertised
            .pointer("/capabilities/diagnosticProvider/workspaceDiagnostics")
            .filter(|value| *value == &Value::Bool(true)),
        Some(&Value::Bool(true)),
        "pull diagnostics advertised for the watched root"
    );
}

/// The first streamed partial names the document it reports.
pub fn assert_partial_document(value: &Value) {
    assert!(
        value
            .get(0)
            .and_then(|report| report.get("uri"))
            .is_some_and(Value::is_string),
        "partial carries its document"
    );
}

/// The `result` of a successful response; wire `null` reads as `None` in
/// `Option<Value>`, so both forms normalize to JSON null here.
pub fn result_of(message: &Message) -> Value {
    message.result.clone().unwrap_or(Value::Null)
}

/// The `error` of a failed response; absence is a failed extraction step.
pub fn error_of(message: &Message) -> StepResult<JsonRpcError> {
    message
        .error
        .clone()
        .ok_or_else(|| format!("response carries an error: {message:?}").into())
}

/// Native read failures remain informative errors rather than successful absence.
pub fn assert_backend_failure(message: &Message) {
    assert!(
        message.result.is_none(),
        "a failed read cannot report success"
    );
    match message.error.clone() {
        Some(error) => {
            assert_eq!(error.code, -32603);
            assert!(
                error.message.contains("read failed"),
                "honest backend failure: {error:?}"
            );
        }
        None => assert!(
            message.error.is_some(),
            "response carries an error: {message:?}"
        ),
    }
}

/// Parse the `p:…|d:…|g:…` snapshot identity into its fields.
pub fn identity(result_id: &str) -> StepResult<(u64, u64, u64)> {
    let fields = result_id
        .split('|')
        .map(identity_field)
        .collect::<StepResult<Vec<_>>>()?;
    match fields.as_slice() {
        [revision, version, generation] => Ok((*revision, *version, *generation)),
        _ => Err(format!("snapshot identity has three fields: {result_id}").into()),
    }
}

/// The numeric value after a field's colon, e.g. `d:12` → 12.
fn identity_field(field: &str) -> StepResult<u64> {
    field
        .split(':')
        .nth(1)
        .ok_or_else(|| format!("identity field follows its colon: {field}").into())
        .and_then(|value| {
            value
                .parse::<u64>()
                .map_err(|error| format!("snapshot identity field is numeric: {error}").into())
        })
}

/// The snapshot identity of a report as `(revision, version, generation)`.
pub fn report_identity(report: &Value) -> StepResult<(u64, u64, u64)> {
    identity(
        report
            .get("resultId")
            .and_then(Value::as_str)
            .ok_or("resultId")?,
    )
}

/// The snapshot generation a streamed partial reports.
pub fn partial_generation(partial: &Value) -> Option<u64> {
    partial
        .get(0)
        .and_then(|report| report_identity(report).ok())
        .map(|(_, _, generation)| generation)
}

/// The file URI for a path inside a fixture workspace.
pub fn uri_for(path: impl AsRef<Path>) -> StepResult<String> {
    Ok(path_to_uri(path.as_ref())?)
}

/// Resolve the exact prebuilt producer; never select a stale artifact by time.
pub fn reference_binary() -> StepResult<PathBuf> {
    Ok(reference_program::reference_binary()?)
}

/// A workspace fixture with `count` clean JSON files at the watched root.
pub fn bulk_workspace(count: usize) -> StepResult<TempDir> {
    let workspace = TempDir::new()?;
    for index in 0..count {
        let name = format!("scan-{index:04}.json");
        std::fs::write(workspace.path().join(name), format!("{{\"n\": {index}}}"))?;
    }
    Ok(workspace)
}

/// Initialize a session rooted at `root` and return the advertisement.
pub async fn initialize_at(client: &mut ReferenceClient, root: &Path) -> StepResult<Value> {
    let root_uri = path_to_uri(root)?;
    let initialized = client
        .request(
            "initialize",
            json!({ "processId": Value::Null, "rootUri": root_uri, "capabilities": {} }),
        )
        .await?;
    let advertised = result_of(&initialized);
    client
        .notify("notifications/initialized", json!({}))
        .await?;
    Ok(advertised)
}
