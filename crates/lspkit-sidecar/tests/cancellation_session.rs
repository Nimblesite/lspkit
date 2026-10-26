//! Actual native replies retain their request identity through cancellation.
//! [LSP-PULL-CANCELLATION] [LSP-PULL-TRANSPORT]

use lspkit_sidecar::{cancellation, inbound, write_frame, DEFAULT_MAX_FRAME};
use serde_json::{json, Value};
use std::path::Path;
use tokio::process::{Child, ChildStdin};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use common::{at, entry};

mod common;
#[path = "../../../test-support/reference_program.rs"]
mod reference_program;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type CloseOutcome = (std::process::ExitStatus, bool, bool);
const INVALID: &str = "{\"unterminated\": ";

struct NativeSession {
    child: Child,
    input: Option<ChildStdin>,
    inbound: inbound::Inbound<Value>,
    id: u64,
}

impl NativeSession {
    fn start() -> TestResult<Self> {
        let mut child = reference_program::spawn_reference(&["--backend"])?;
        let input = child.stdin.take().ok_or("backend stdin missing")?;
        let output = child.stdout.take().ok_or("backend stdout missing")?;
        let inbound = inbound::spawn(
            output,
            DEFAULT_MAX_FRAME,
            |bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
            |envelope: &Value| envelope.get("id").is_none(),
        );
        Ok(Self {
            child,
            input: Some(input),
            inbound,
            id: 0,
        })
    }

    async fn send(&mut self, envelope: Value) -> Result<(), String> {
        let bytes = serde_json::to_vec(&envelope).map_err(|error| error.to_string())?;
        let input = self.input.as_mut().ok_or("backend stdin closed")?;
        write_frame(input, &bytes)
            .await
            .map_err(|error| error.to_string())
    }

    async fn analyze(&mut self, id: u64, path: &Path) -> TestResult {
        self.id = id;
        self.send(json!({"id": id, "method": "analyze", "params": {"path": path}}))
            .await?;
        Ok(())
    }

    async fn response(
        &mut self,
        token: Option<&CancellationToken>,
        cancelled: bool,
    ) -> TestResult<Value> {
        let request = cancellation::receive(self, token);
        let reply = timeout(reference_program::READ_GUARD, request).await??;
        match (reply, cancelled) {
            (cancellation::Reply::Complete(value), false)
            | (cancellation::Reply::Cancelled(value), true) => Ok(value),
            _ => Err("terminal reply has the wrong cancellation disposition".into()),
        }
    }

    async fn close(&mut self) -> TestResult<CloseOutcome> {
        drop(self.input.take());
        let status = timeout(reference_program::READ_GUARD, self.child.wait()).await??;
        timeout(reference_program::READ_GUARD, &mut self.inbound.task).await??;
        let responses_closed = self.inbound.responses.recv().await.is_none();
        let notifications_closed = self.inbound.notifications.recv().await.is_none();
        Ok((status, responses_closed, notifications_closed))
    }
}

impl cancellation::Exchange for NativeSession {
    type Response = Value;

    async fn receive(&mut self) -> Result<Value, String> {
        self.inbound
            .responses
            .recv()
            .await
            .ok_or_else(|| "backend response stream ended".to_owned())?
    }

    async fn cancel(&mut self) -> Result<(), String> {
        self.send(json!({"method": "$/cancelRequest", "params": {"id": self.id}}))
            .await
    }
}

impl Drop for NativeSession {
    fn drop(&mut self) {
        if let Err(error) = self.child.start_kill() {
            tracing::warn!(%error, "native test backend cleanup failed");
        }
    }
}

/// The real backend exits naturally and closes both ordered event streams.
fn assert_close((status, responses_closed, notifications_closed): CloseOutcome) {
    assert!(status.success(), "real backend exits naturally: {status:?}");
    assert!(responses_closed);
    assert!(notifications_closed);
}

/// Replies retain identity, source location and diagnostics for the active request.
fn assert_reply(value: &Value, id: u64, path: &Path, broken: bool) {
    assert_eq!(
        at(value, "/id"),
        id,
        "only the current request owns this reply"
    );
    assert_eq!(at(value, "/result/path"), &json!(path));
    assert!(value.get("error").is_none());
    let diagnostics = at(value, "/result/diagnostics").as_array();
    assert_eq!(diagnostics.map_or(0, Vec::len), usize::from(broken));
    if broken {
        let diagnostic = diagnostics.map_or(&Value::Null, |items| entry(items, 0));
        assert_parse_error(diagnostic);
    }
}

/// The backend reports the actual parser's location, severity, origin and message.
fn assert_parse_error(diagnostic: &Value) {
    let actual = match serde_json::from_str::<Value>(INVALID) {
        Err(error) => error,
        Ok(parsed) => {
            assert_eq!(parsed, Value::Null, "invalid source parsed");
            return;
        }
    };
    assert_eq!(
        at(diagnostic, "/message").as_str().unwrap_or_default(),
        actual.to_string()
    );
    assert_eq!(at(diagnostic, "/severity"), 1);
    assert_eq!(at(diagnostic, "/source"), "serde_json");
    assert_eq!(
        at(diagnostic, "/range/start"),
        &json!({"line": actual.line() - 1, "character": actual.column() - 1})
    );
}

fn source_session() -> TestResult<(
    tempfile::TempDir,
    std::path::PathBuf,
    NativeSession,
    CancellationToken,
)> {
    let fixture = tempfile::TempDir::new()?;
    let path = fixture.path().join("source.json");
    let session = NativeSession::start()?;
    Ok((fixture, path, session, CancellationToken::new()))
}

#[tokio::test]
async fn actual_diagnostics_repair_and_regress_with_uncancelled_interest() {
    let (_fixture, path, mut session, token) = source_session().expect("start native session");
    for (id, contents, broken) in [(1, INVALID, true), (2, "{}", false), (3, INVALID, true)] {
        std::fs::write(&path, contents).expect("write native source");
        session.analyze(id, &path).await.expect("request analysis");
        let interest = (id != 1).then_some(&token);
        let reply = session
            .response(interest, false)
            .await
            .expect("native reply");
        assert_reply(&reply, id, &path, broken);
        assert!(!token.is_cancelled());
    }
    assert_close(session.close().await.expect("close native session"));
}

#[tokio::test]
async fn cancelled_reply_is_drained_before_the_actual_repaired_file_is_queried() {
    let (_fixture, path, mut session, token) = source_session().expect("start native session");
    token.cancel();
    std::fs::write(&path, INVALID).expect("write invalid source");
    session
        .analyze(41, &path)
        .await
        .expect("request cancelled analysis");
    let retired = session
        .response(Some(&token), true)
        .await
        .expect("retired reply");
    assert_reply(&retired, 41, &path, true);
    std::fs::write(&path, "{}").expect("repair source");
    session
        .analyze(42, &path)
        .await
        .expect("request repaired analysis");
    let repaired = session.response(None, false).await.expect("repaired reply");
    assert_reply(&repaired, 42, &path, false);
    assert_close(session.close().await.expect("close native session"));
}

#[tokio::test]
async fn ended_real_backend_is_an_error_with_and_without_cancellation() {
    let mut session = NativeSession::start().expect("start native session");
    assert_close(session.close().await.expect("close native session"));
    let token = CancellationToken::new();
    token.cancel();
    let normal = (None, false, "backend response stream ended");
    let cancelled = (Some(&token), true, "backend stdin closed");
    for (interest, disposition, expected) in [normal, cancelled] {
        let response = session.response(interest, disposition).await;
        let error = response.err().map(|error| error.to_string());
        assert_eq!(error.as_deref(), Some(expected));
    }
}

fn native_watcher(root: &Path) -> TestResult<lspkit_sidecar::Sidecar> {
    let program = reference_program::reference_binary()?;
    let args = vec![
        "--watch".to_owned(),
        root.display().to_string(),
        "0".to_owned(),
    ];
    Ok(lspkit_sidecar::Sidecar::new(
        program.to_str().ok_or("backend path is not UTF8")?,
        args,
        lspkit_sidecar::LifecycleConfig::default(),
    ))
}

#[tokio::test]
async fn native_watcher_lifecycle_reaps_and_respawns_the_actual_process() {
    let fixture = tempfile::TempDir::new().expect("native watcher fixture");
    let mut sidecar = native_watcher(fixture.path()).expect("configure native watcher");
    assert!(sidecar.wait().await.is_none());
    for _ in 0..2 {
        sidecar.spawn().expect("spawn native watcher");
        let status = timeout(reference_program::READ_GUARD, sidecar.wait())
            .await
            .expect("watcher wait remains responsive")
            .expect("spawned child was lost");
        assert!(status.success(), "actual watcher failed: {status:?}");
        assert_eq!(sidecar.wait().await, Some(status));
        assert_eq!(sidecar.next_backoff(), std::time::Duration::from_secs(1));
    }
}
