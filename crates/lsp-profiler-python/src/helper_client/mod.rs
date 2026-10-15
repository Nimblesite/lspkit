//! Language-server side of the elevated-helper sampling path.
//! Implements [PROF-HELPER-SOCKET].
//!
//! On macOS, py-spy cannot `vm_read` a process the language server did not
//! launch as a child unless it runs as root. The server therefore:
//!
//! 1. binds a [`UnixListener`] **before** spawning the helper (issue #61,
//!    Defect 1 — previously nothing ever listened, so the helper's
//!    `connect()` always failed with `No such file or directory`);
//! 2. spawns `lsp-profiler-helper` — elevated via `osascript` in
//!    production, or directly for tests — without blocking on its exit
//!    (Defect 3 — the helper is a long-lived streamer, not a one-shot);
//! 3. accepts the connection, drives the `attach`/`samples`/`stop` protocol
//!    ([`lsp_profiler::wire::HelperCommand`] / [`lsp_profiler::wire::HelperMessage`]),
//!    and forwards samples into the same
//!    [`SamplerHandle`] channel the in-process sampler uses, so the rest of the
//!    pipeline is identical for both sources.
//!
//! The elevated `osascript` command is cwd-guarded (Defect 2) so it never
//! inherits an inaccessible working directory.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use lsp_profiler::ndjson::{read_message, write_message};
use lsp_profiler::sampler::{SampleBatch, SamplerError, SamplerHandle, SamplerSink};
use lsp_profiler::wire::{AttachErrorKind, HelperCommand, HelperMessage};
use lsp_profiler::SamplerConfig;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixListener;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{info, warn};

use crate::attach_errors::classify_attach_error;
use crate::RUNTIME_DISPLAY_NAME;

mod launch;
pub use launch::build_elevation_script;
use launch::create_socket_path;

/// How long to wait for the helper to connect back to the bound socket.
const ACCEPT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long to wait for the helper's `attached` confirmation after `attach`.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
/// How often the driver checks whether a stop was requested.
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How long to wait for the helper's final `stopped` after we send `stop`.
const STOP_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for a dead helper's exit status and stderr while
/// diagnosing a failed handshake (issue #81).
const EXIT_DIAGNOSIS_TIMEOUT: Duration = Duration::from_secs(3);
/// How many trailing stderr lines to surface in a handshake diagnosis.
const STDERR_TAIL_LINES: usize = 5;
/// File name of the helper binary.
pub const HELPER_BINARY: &str = "lsp-profiler-helper";

/// Where to find the elevated helper binary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelperLocation {
    /// An explicit path configured by the host (e.g. a bundled binary).
    pub explicit: Option<PathBuf>,
}

impl HelperLocation {
    /// The explicit path, else a binary next to the running executable, else `PATH`.
    ///
    /// # Errors
    ///
    /// Returns [`SamplerError::PermissionDenied`] naming the binary when none is found.
    pub fn resolve(&self) -> Result<PathBuf, SamplerError> {
        let adjacent = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(HELPER_BINARY)));
        let on_path = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).map(|dir| dir.join(HELPER_BINARY)).collect::<Vec<_>>())
            .unwrap_or_default();
        self.explicit
            .iter()
            .cloned()
            .chain(adjacent)
            .chain(on_path)
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| {
                SamplerError::PermissionDenied(format!(
                    "could not find {HELPER_BINARY}; install it next to the language server or configure its path"
                ))
            })
    }
}

/// How the helper binary should be launched.
#[derive(Debug)]
pub enum HelperSpawn {
    /// Launch this helper with administrator privileges via `osascript` (macOS only).
    Elevated(PathBuf),
    /// Launch the given binary directly, without elevation (used by tests and
    /// any environment where the caller already has sufficient privileges).
    Direct(PathBuf),
}

/// Start a profiling sampler backed by the elevated helper over a Unix socket.
///
/// Binds the listener, spawns the helper, performs the attach handshake, and
/// returns a [`SamplerHandle`] whose channel streams samples for as long as the
/// handle lives. Dropping or [`SamplerHandle::stop`]-ing the handle tells the
/// helper to stop and tears down the socket.
///
/// # Errors
///
/// Returns a [`SamplerError`] if the socket cannot be bound, the helper cannot
/// be spawned or elevated, or the attach handshake fails or times out.
pub async fn start_helper_sampler(
    config: &SamplerConfig,
    spawn: HelperSpawn,
) -> Result<SamplerHandle, SamplerError> {
    let (ready_tx, ready_rx) = oneshot::channel::<Result<String, SamplerError>>();
    let (sink_tx, sink_rx) = oneshot::channel::<(SamplerSink, oneshot::Sender<()>)>();
    let driver = DriverArgs {
        socket_path: create_socket_path(config.pid),
        spawn,
        pid: config.pid,
        rate: config.sample_rate,
        native: config.include_native,
        ready_tx,
        sink_rx,
    };
    let join_handle = std::thread::Builder::new()
        .name(format!("profiler-helper-{}", config.pid))
        .spawn(move || run_driver_thread(driver))
        .map_err(|err| SamplerError::AttachFailed(format!("failed to spawn helper driver thread: {err}")))?;
    let ready = ready_rx.await;
    match ready {
        Ok(Ok(runtime_version)) => {
            let parts = SamplerHandle::channel(config.pid, runtime_version, STOP_DRAIN_TIMEOUT + Duration::from_secs(1));
            sink_tx
                .send((parts.sink, parts.done))
                .map_err(|_unsent| SamplerError::AttachFailed("helper driver exited after attach".to_owned()))?;
            Ok(parts.handle)
        }
        Ok(Err(err)) => {
            let _ = join_handle.join();
            Err(err)
        }
        Err(_closed) => {
            let _ = join_handle.join();
            Err(SamplerError::AttachFailed("helper driver exited before reporting readiness".to_owned()))
        }
    }
}

/// Everything the driver thread needs to run the socket protocol.
struct DriverArgs {
    socket_path: PathBuf,
    spawn: HelperSpawn,
    pid: u32,
    rate: u64,
    native: bool,
    ready_tx: oneshot::Sender<Result<String, SamplerError>>,
    sink_rx: oneshot::Receiver<(SamplerSink, oneshot::Sender<()>)>,
}

/// Entry point for the dedicated driver thread: builds a single-threaded
/// runtime and drives the helper to completion.
fn run_driver_thread(args: DriverArgs) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = args.ready_tx.send(Err(SamplerError::AttachFailed(format!(
                "failed to build helper runtime: {err}"
            ))));
            return;
        }
    };
    runtime.block_on(drive_helper(args));
}

/// A live, attached helper connection.
struct HelperConn {
    child: tokio::process::Child,
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    runtime_version: String,
}

/// Bind, spawn, accept, handshake, then stream until stop — cleaning up after.
async fn drive_helper(args: DriverArgs) {
    let DriverArgs { socket_path, spawn, pid, rate, native, ready_tx, sink_rx } = args;
    // A stale socket file from a crashed run would make bind fail with EADDRINUSE.
    let _ = std::fs::remove_file(&socket_path);
    match handshake(&socket_path, &spawn, pid, rate, native).await {
        Err(err) => {
            let _ = ready_tx.send(Err(err));
        }
        Ok(mut conn) => {
            let sink = if ready_tx.send(Ok(conn.runtime_version.clone())).is_ok() { sink_rx.await.ok() } else { None };
            if let Some((sink, done)) = sink {
                stream_samples(&mut conn.reader, &mut conn.writer, &sink).await;
                let _ = done.send(());
            }
            let _ = conn.child.start_kill();
            let _ = conn.child.wait().await;
        }
    }
    let _ = std::fs::remove_file(&socket_path);
}

/// Bind the socket, launch the helper, and complete the attach handshake.
async fn handshake(
    socket_path: &Path,
    spawn: &HelperSpawn,
    pid: u32,
    rate: u64,
    native: bool,
) -> Result<HelperConn, SamplerError> {
    // Bind BEFORE spawning so the helper's connect() always finds a listener.
    let listener = UnixListener::bind(socket_path).map_err(|err| {
        SamplerError::AttachFailed(format!(
            "failed to bind profiler socket {}: {err}",
            socket_path.display()
        ))
    })?;

    let child = spawn_helper(spawn, socket_path)?;

    let (stream, _addr) = match timeout(ACCEPT_TIMEOUT, listener.accept()).await {
        Ok(Ok(accepted)) => accepted,
        Ok(Err(err)) => {
            return Err(SamplerError::AttachFailed(format!(
                "accept on profiler socket failed: {err}"
            )))
        }
        Err(_elapsed) => {
            // The helper never connected — e.g. the user dismissed the macOS
            // elevation prompt. Its exit status/stderr say why (issue #81).
            let diagnosis = helper_exit_diagnosis(child).await;
            return Err(SamplerError::AttachFailed(format!(
                "timed out waiting for the profiler helper to connect ({diagnosis})"
            )));
        }
    };

    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut writer = write_half;

    let attach = HelperCommand::Attach {
        pid: i64::from(pid),
        sample_rate: i64::try_from(rate).unwrap_or(i64::MAX),
        include_native: native,
    };
    if let Err(err) = write_message(&mut writer, &attach).await {
        // A send failure (e.g. EPIPE) means the helper hung up mid-handshake —
        // it died before confirming attach, so reap it and surface its exit
        // status and stderr exactly like the EOF path below (issue #81).
        let diagnosis = helper_exit_diagnosis(child).await;
        let message = format!("failed to send attach command: {err} ({diagnosis})");
        return Err(match classify_attach_error(&message) {
            AttachErrorKind::PermissionDenied => SamplerError::PermissionDenied(message),
            _ => SamplerError::AttachFailed(message),
        });
    }

    let runtime_version =
        match timeout(HANDSHAKE_TIMEOUT, read_message::<_, HelperMessage>(&mut reader)).await {
            Ok(Ok(Some(HelperMessage::Attached { runtime_version, .. }))) => runtime_version,
            // The helper reported WHY it failed — surface the real, classified
            // cause instead of a generic attach error (issue #81).
            Ok(Ok(Some(HelperMessage::Failure { error_kind, message }))) => {
                return Err(SamplerError::from_kind(&error_kind, pid, RUNTIME_DISPLAY_NAME, message))
            }
            Ok(Ok(Some(HelperMessage::Samples { .. } | HelperMessage::Stopped))) => {
                return Err(SamplerError::AttachFailed(
                    "helper sent an unexpected message before confirming attach".to_owned(),
                ))
            }
            Ok(Ok(None)) => {
                // Bare EOF (e.g. an old helper, or one killed before it could
                // report) — reap it and surface exit status + stderr (issue #81).
                let diagnosis = helper_exit_diagnosis(child).await;
                let message =
                    format!("helper closed the connection before confirming attach ({diagnosis})");
                return Err(match classify_attach_error(&message) {
                    AttachErrorKind::PermissionDenied => SamplerError::PermissionDenied(message),
                    _ => SamplerError::AttachFailed(message),
                });
            }
            Ok(Err(err)) => {
                // A read error (e.g. ECONNRESET) races bare EOF when the
                // helper dies mid-handshake — reap it and surface its exit
                // status and stderr exactly like the EOF path above, so the
                // user still sees WHY (issue #81).
                let diagnosis = helper_exit_diagnosis(child).await;
                let message = format!("attach handshake failed: {err} ({diagnosis})");
                return Err(match classify_attach_error(&message) {
                    AttachErrorKind::PermissionDenied => SamplerError::PermissionDenied(message),
                    _ => SamplerError::AttachFailed(message),
                });
            }
            Err(_elapsed) => {
                let diagnosis = helper_exit_diagnosis(child).await;
                return Err(SamplerError::AttachFailed(format!(
                    "timed out waiting for the helper to confirm attach ({diagnosis})"
                )));
            }
        };

    info!(pid, %runtime_version, "elevated helper attached");
    Ok(HelperConn {
        child,
        reader,
        writer,
        runtime_version,
    })
}

/// Reap a helper that failed the handshake and describe its exit status and
/// trailing stderr — the diagnosable cause issue #81 demanded in place of a
/// bare EOF. Kills the helper if it refuses to exit within the bounded wait.
/// Implements [PROFILE-HELPER-PROTOCOL-ERRORS] (exit-diagnosis fallback).
async fn helper_exit_diagnosis(mut child: tokio::process::Child) -> String {
    let mut stderr_text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = timeout(
            EXIT_DIAGNOSIS_TIMEOUT,
            stderr.read_to_string(&mut stderr_text),
        )
        .await;
    }
    let status = match timeout(EXIT_DIAGNOSIS_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => format!("helper exited with {status}"),
        Ok(Err(err)) => format!("helper exit status unavailable: {err}"),
        Err(_elapsed) => {
            let _ = child.start_kill();
            "helper did not exit".to_owned()
        }
    };
    let tail = stderr_tail(&stderr_text);
    if tail.is_empty() {
        status
    } else {
        format!("{status}; stderr: {tail}")
    }
}

/// The last few non-empty stderr lines, joined for a single-line diagnosis.
fn stderr_tail(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let skip = lines.len().saturating_sub(STDERR_TAIL_LINES);
    lines
        .iter()
        .skip(skip)
        .copied()
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Spawn the helper process per the requested mode (detached — never awaited).
///
/// `tokio::process::Command::spawn` returns immediately (it does not await the
/// child), so this is synchronous; it must still run inside a Tokio runtime,
/// which the driver thread provides.
fn spawn_helper(
    spawn: &HelperSpawn,
    socket_path: &Path,
) -> Result<tokio::process::Child, SamplerError> {
    match spawn {
        // Stderr is piped so a failed handshake can surface the helper's own
        // error output instead of an undiagnosable EOF (issue #81).
        HelperSpawn::Direct(path) => tokio::process::Command::new(path)
            .arg(socket_path)
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| {
                SamplerError::AttachFailed(format!(
                    "failed to spawn profiler helper {}: {err}",
                    path.display()
                ))
            }),
        HelperSpawn::Elevated(helper) => spawn_elevated(helper, socket_path),
    }
}

/// Launch the helper with administrator privileges via `osascript` (macOS).
#[cfg(target_os = "macos")]
fn spawn_elevated(helper: &Path, socket_path: &Path) -> Result<tokio::process::Child, SamplerError> {
    let script = build_elevation_script(
        &helper.display().to_string(),
        &socket_path.display().to_string(),
    );
    info!(helper = %helper.display(), "spawning elevated profiler helper via osascript");
    // Stderr is piped so an elevation failure (e.g. the user cancelling the
    // privilege prompt) is surfaced with osascript's own message (issue #81).
    tokio::process::Command::new("osascript")
        .args(["-e", &script])
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| {
            SamplerError::PermissionDenied(format!(
                "failed to spawn osascript for privilege elevation: {err}"
            ))
        })
}

/// Non-macOS fallback: elevation via the helper is macOS-only.
#[cfg(not(target_os = "macos"))]
fn spawn_elevated(_helper: &Path, _socket_path: &Path) -> Result<tokio::process::Child, SamplerError> {
    Err(SamplerError::PermissionDenied(
        "Privilege elevation via the profiler helper is only supported on macOS".to_owned(),
    ))
}

/// Stream samples from the helper, forwarding them to the sampler channel until
/// the helper stops on its own or a stop is requested.
///
/// The read side is never cancelled mid-message (newline framing is not
/// cancellation-safe): the stop signal is handled on the write side, and the
/// reader is then awaited to completion to drain the final `stopped`.
async fn stream_samples(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    sink: &SamplerSink,
) {
    let read_loop = read_until_stopped(reader, sink);
    tokio::pin!(read_loop);

    let send_stop = async {
        let mut poll = tokio::time::interval(STOP_POLL_INTERVAL);
        loop {
            let _ = poll.tick().await;
            if sink.stop_requested() {
                if let Err(err) = write_message(writer, &HelperCommand::Stop).await {
                    warn!(%err, "failed to send stop command to helper");
                }
                break;
            }
        }
    };

    tokio::select! {
        () = &mut read_loop => {}
        () = send_stop => {
            // We asked the helper to stop; drain until it confirms (bounded).
            let _ = timeout(STOP_DRAIN_TIMEOUT, read_loop).await;
        }
    }
}

/// Read messages until the helper reports `stopped`, closes, or the receiver
/// is gone. Forwards each `samples` batch to the sampler channel.
async fn read_until_stopped(reader: &mut BufReader<OwnedReadHalf>, sink: &SamplerSink) {
    loop {
        match read_message::<_, HelperMessage>(reader).await {
            Ok(Some(HelperMessage::Samples { samples })) => {
                if sink.sender.send(SampleBatch::new(samples)).await.is_err() {
                    break;
                }
            }
            Ok(Some(HelperMessage::Stopped) | None) => break,
            Ok(Some(HelperMessage::Failure { error_kind, message })) => {
                warn!(?error_kind, %message, "helper reported an error during sampling");
                break;
            }
            Ok(Some(HelperMessage::Attached { .. })) => {}
            Err(err) => {
                warn!(%err, "error reading from profiler helper");
                break;
            }
        }
    }
}
