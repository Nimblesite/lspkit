//! Native CLI consumer of the real framed analysis backend. [LSP-PULL-TRANSPORT]
//! Accepts newline JSON requests; emits typed response/notification/error events.

use std::process::{ExitCode, ExitStatus, Stdio};

use lspkit_sidecar::{inbound, write_frame, TransportError, DEFAULT_MAX_FRAME};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::task::{JoinError, JoinHandle};

use crate::client_wire::{self, Closed, Packet};

/// Startup, transport and task failures cannot masquerade as clean EOF.
#[derive(Debug, thiserror::Error)]
pub(super) enum ClientError {
    /// Native stdin/stdout/process I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The shared framing layer rejected a frame.
    #[error(transparent)]
    Frame(#[from] TransportError),
    /// A typed envelope or journal event could not be rendered.
    #[error(transparent)]
    Render(#[from] client_wire::WireError),
    /// A spawned consumer task failed or was cancelled.
    #[error(transparent)]
    Task(#[from] JoinError),
    /// The optional frame ceiling did not parse as an integer.
    #[error(transparent)]
    Limit(#[from] std::num::ParseIntError),
    /// The native filesystem watcher failed to start or run.
    #[error(transparent)]
    Watch(#[from] lspkit_live::watcher::WatcherError),
    /// A required child-process pipe was unavailable.
    #[error("backend {0} pipe was unavailable")]
    Pipe(&'static str),
    /// The backend child process exited unsuccessfully.
    #[error("backend exited with {0}")]
    Backend(ExitStatus),
}

/// Process and task ownership is confined to this consumer session.
struct Session {
    /// The real backend child process, waited on at session end.
    child: Child,
    /// The stdin forwarder, cancelled once the reader terminates.
    forwarding: JoinHandle<Result<(), ClientError>>,
    /// The shared inbound reader with independently closed channels.
    inbound: inbound::Inbound<Packet>,
}

impl Session {
    /// Spawn the same compiled program's actual backend over native OS pipes.
    fn start(max_frame: usize) -> Result<Self, ClientError> {
        let mut child = spawn_backend()?;
        let input = child.stdin.take().ok_or(ClientError::Pipe("stdin"))?;
        let output = child.stdout.take().ok_or(ClientError::Pipe("stdout"))?;
        let inbound = inbound::spawn(
            output,
            max_frame,
            |payload| client_wire::decode(&payload),
            client_wire::is_notification,
        );
        Ok(Self {
            child,
            forwarding: tokio::spawn(forward(input)),
            inbound,
        })
    }

    /// Drain native traffic, then join the reader and naturally exiting backend.
    async fn run(mut self) -> Result<(), ClientError> {
        let mut closed = Closed::default();
        let rendered = client_wire::render(&mut self.inbound, &mut closed).await;
        self.forwarding.abort();
        let forwarded = forwarded(self.forwarding.await);
        let reader = self.inbound.task.await;
        let status = self.child.wait().await?;
        client_wire::finish(closed, reader.is_ok(), Some(status.success())).await?;
        rendered?;
        forwarded?;
        reader?;
        if status.success() {
            Ok(())
        } else {
            Err(ClientError::Backend(status))
        }
    }
}

/// Start the real framed producer with native pipes and inherited diagnostics.
fn spawn_backend() -> Result<Child, ClientError> {
    Ok(Command::new(std::env::current_exe()?)
        .arg("--backend")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?)
}

/// Forward raw lines; malformed JSON is processed by the actual backend.
async fn forward(mut input: ChildStdin) -> Result<(), ClientError> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        write_frame(&mut input, line.as_bytes()).await?;
    }
    Ok(())
}

/// Cancel only our own pending stdin forwarder after a terminal reader event.
fn forwarded(outcome: Result<Result<(), ClientError>, JoinError>) -> Result<(), ClientError> {
    match outcome {
        Ok(result) => result,
        Err(error) if error.is_cancelled() => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// The optional frame ceiling is a normal CLI argument, not a test-only switch.
fn frame_limit() -> Result<usize, ClientError> {
    std::env::args()
        .nth(2)
        .map_or(Ok(DEFAULT_MAX_FRAME), |value| Ok(value.parse()?))
}

/// Run one consumer session, reporting protocol output separately from logs.
pub(super) async fn serve() -> ExitCode {
    let outcome = match frame_limit().and_then(Session::start) {
        Ok(session) => session.run().await,
        Err(error) => Err(error),
    };
    exit(outcome, "native_client")
}

/// CLI modes share one structured failure boundary, separate from rendered data.
pub(super) fn exit(outcome: Result<(), impl std::fmt::Display>, mode: &'static str) -> ExitCode {
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(mode, %error, "reference IPC consumer failed");
            ExitCode::FAILURE
        }
    }
}
