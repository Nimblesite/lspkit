//! Actual native replies retain their request identity through cancellation.
//! [LSP-PULL-CANCELLATION] [LSP-PULL-TRANSPORT]

use lspkit_sidecar::{cancellation, inbound, write_frame, DEFAULT_MAX_FRAME};
use serde_json::{json, Value};
use std::path::Path;
use tokio::process::{Child, ChildStdin};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

#[path = "../../../test-support/reference_program.rs"]
mod reference_program;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
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

    async fn close(&mut self) -> TestResult {
        drop(self.input.take());
        let status = timeout(reference_program::READ_GUARD, self.child.wait()).await??;
        assert!(status.success(), "real backend exits naturally: {status:?}");
        timeout(reference_program::READ_GUARD, &mut self.inbound.task).await??;
        assert!(self.inbound.responses.recv().await.is_none());
        assert!(self.inbound.notifications.recv().await.is_none());
        Ok(())
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

fn assert_reply(value: &Value, id: u64, path: &Path, broken: bool) -> TestResult {
    assert_eq!(value["id"], id, "only the current request owns this reply");
    assert_eq!(value["result"]["path"], json!(path));
    assert!(value.get("error").is_none());
    let diagnostics = value["result"]["diagnostics"]
        .as_array()
        .ok_or("diagnostics missing")?;
    assert_eq!(diagnostics.len(), usize::from(broken));
    if broken {
        assert_parse_error(&diagnostics[0])?;
    }
    Ok(())
}

fn assert_parse_error(diagnostic: &Value) -> TestResult {
    let actual = serde_json::from_str::<Value>(INVALID)
        .err()
        .ok_or("invalid source parsed")?;
    assert_eq!(diagnostic["message"], actual.to_string());
    assert_eq!(diagnostic["severity"], 1);
    assert_eq!(diagnostic["source"], "serde_json");
    assert_eq!(
        diagnostic["range"]["start"],
        json!({"line": actual.line() - 1, "character": actual.column() - 1})
    );
    Ok(())
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
async fn actual_diagnostics_repair_and_regress_with_uncancelled_interest() -> TestResult {
    let (_fixture, path, mut session, token) = source_session()?;
    for (id, contents, broken) in [(1, INVALID, true), (2, "{}", false), (3, INVALID, true)] {
        std::fs::write(&path, contents)?;
        session.analyze(id, &path).await?;
        let interest = (id != 1).then_some(&token);
        assert_reply(&session.response(interest, false).await?, id, &path, broken)?;
        assert!(!token.is_cancelled());
    }
    session.close().await
}

#[tokio::test]
async fn cancelled_reply_is_drained_before_the_actual_repaired_file_is_queried() -> TestResult {
    let (_fixture, path, mut session, token) = source_session()?;
    token.cancel();
    std::fs::write(&path, INVALID)?;
    session.analyze(41, &path).await?;
    let retired = session.response(Some(&token), true).await?;
    assert_reply(&retired, 41, &path, true)?;
    std::fs::write(&path, "{}")?;
    session.analyze(42, &path).await?;
    assert_reply(&session.response(None, false).await?, 42, &path, false)?;
    session.close().await
}

#[tokio::test]
async fn ended_real_backend_is_an_error_with_and_without_cancellation() -> TestResult {
    let mut session = NativeSession::start()?;
    session.close().await?;
    let token = CancellationToken::new();
    token.cancel();
    let normal = (None, false, "backend response stream ended");
    let cancelled = (Some(&token), true, "backend stdin closed");
    for (interest, disposition, expected) in [normal, cancelled] {
        let response = session.response(interest, disposition).await;
        let error = response
            .err()
            .ok_or("terminal failure fabricated a reply")?;
        assert_eq!(error.to_string(), expected);
    }
    Ok(())
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
async fn native_watcher_lifecycle_reaps_and_respawns_the_actual_process() -> TestResult {
    let fixture = tempfile::TempDir::new()?;
    let mut sidecar = native_watcher(fixture.path())?;
    assert!(sidecar.wait().await.is_none());
    for _ in 0..2 {
        sidecar.spawn()?;
        let status = timeout(reference_program::READ_GUARD, sidecar.wait())
            .await?
            .ok_or("spawned child was lost")?;
        assert!(status.success(), "actual watcher failed: {status:?}");
        assert_eq!(sidecar.wait().await, Some(status));
        assert_eq!(sidecar.next_backoff(), std::time::Duration::from_secs(1));
    }
    Ok(())
}
