//! Lifetime regressions for a failed `memory/start`. [PROF-MEMORY-START]
//!
//! `memory/start` opens the session before it runs the backend's first step.
//! When that step fails the client never receives the session id, so nothing
//! outside the manager can stop it: the failed start must release the session
//! itself, the way `memory/stop` and shutdown do.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{ProfilerConfig, ProfilerEvents, ProfilerService};
use crate::error::ProfilerError;
use crate::memory::{MemoryAction, MemoryError, MemoryPayload};
use crate::runtime::{
    Classification, CpuBackend, FrameFilter, KeepAllFrames, MemoryBackend, MemoryContext,
    ProcessCandidate, Runtime, TypeQuery,
};
use crate::wire::{methods, MemoryCapabilities, MemoryMode};

/// Runtime identity.
const RUNTIME_ID: &str = "refusing";
/// Failed starts per test: below the manager's session capacity, so eviction
/// can never hide a leaked session.
const ATTEMPTS: usize = 8;
/// Why the backend refuses every start.
const REFUSED: &str = "tracking refused";
/// Where exports from the test service go.
const OUTPUT_DIR: &str = "lsp-profiler-memstart-lifetime";
/// How long the spawned child outlives the test's tracking attempts.
const CHILD_LIFE: &str = "600";

/// A backend that refuses every start and records what it was asked to do.
#[derive(Debug, Default)]
struct Refusing {
    /// Sessions whose start was attempted.
    started: Mutex<Vec<String>>,
    /// Sessions whose stop ran.
    stopped: Mutex<Vec<String>>,
}

impl Refusing {
    /// Sessions whose start was attempted.
    fn starts(&self) -> Vec<String> {
        lock(&self.started).clone()
    }

    /// Sessions whose stop ran.
    fn stops(&self) -> Vec<String> {
        lock(&self.stopped).clone()
    }

    /// Record `session_id` in `list`.
    fn push(list: &Mutex<Vec<String>>, session_id: &str) {
        lock(list).push(session_id.to_owned());
    }
}

/// `list` under its lock, poison-tolerant like the other lifetime tests.
fn lock(list: &Mutex<Vec<String>>) -> MutexGuard<'_, Vec<String>> {
    list.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The answer to every successful step.
const fn ack() -> MemoryAction {
    MemoryAction::Payload(MemoryPayload::Ack)
}

#[async_trait]
impl MemoryBackend for Refusing {
    fn capabilities(&self) -> MemoryCapabilities {
        MemoryCapabilities {
            mode: MemoryMode::Direct,
            allocation_sites: false,
            references: false,
            objects_by_type: false,
            gc_collect: false,
        }
    }
    async fn start(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        Self::push(&self.started, &context.session_id);
        Err(MemoryError::Backend(REFUSED.to_owned()))
    }
    async fn snapshot(&self, _context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        Ok(ack())
    }
    async fn diff(&self, _context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        Ok(ack())
    }
    async fn references(
        &self,
        _context: &MemoryContext,
        _query: &TypeQuery,
    ) -> Result<MemoryAction, MemoryError> {
        Ok(ack())
    }
    async fn objects_by_type(
        &self,
        _context: &MemoryContext,
        _query: &TypeQuery,
    ) -> Result<MemoryAction, MemoryError> {
        Ok(ack())
    }
    async fn gc_collect(&self, _context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        Ok(ack())
    }
    async fn stop(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        Self::push(&self.stopped, &context.session_id);
        Ok(ack())
    }
    async fn parse(
        &self,
        _context: &MemoryContext,
        _output: &str,
    ) -> Result<MemoryPayload, MemoryError> {
        Ok(MemoryPayload::Ack)
    }
}

/// A runtime exposing only the [`Refusing`] backend.
#[derive(Debug)]
struct RefusingRuntime {
    /// The process this runtime claims, so `memory/start` resolves to it.
    pid: u32,
    /// The memory backend.
    backend: Refusing,
}

impl Runtime for RefusingRuntime {
    fn id(&self) -> &'static str {
        RUNTIME_ID
    }
    fn display_name(&self) -> &'static str {
        RUNTIME_ID
    }
    fn classify(&self, candidates: &[ProcessCandidate]) -> Vec<(usize, Classification)> {
        candidates
            .iter()
            .position(|candidate| candidate.pid == self.pid)
            .map(|index| (index, Classification::default()))
            .into_iter()
            .collect()
    }
    fn frame_filter(&self) -> &dyn FrameFilter {
        &KeepAllFrames
    }
    fn cpu(&self) -> Option<&dyn CpuBackend> {
        None
    }
    fn memory(&self) -> Option<&dyn MemoryBackend> {
        Some(&self.backend)
    }
}

/// Events sink that discards notifications.
#[derive(Debug)]
struct Discard;

#[async_trait]
impl ProfilerEvents for Discard {
    async fn notify(&self, _method: &'static str, _params: Value) {}
}

/// A spawned idle process, killed when the test ends. The process table excludes
/// the test binary itself, so the runtime needs another live process to claim.
struct Child(std::process::Child);

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A live process the refusing runtime can claim, on every platform.
fn child() -> Result<Child, String> {
    idle_command()
        .spawn()
        .map(Child)
        .map_err(|error| format!("cannot spawn a child process to track: {error}"))
}

/// A command that idles for [`CHILD_LIFE`] seconds: `sleep` on Unix, and the
/// same wait through PowerShell on Windows, which has no `sleep` binary.
fn idle_command() -> std::process::Command {
    if cfg!(windows) {
        let mut command = std::process::Command::new("powershell.exe");
        let _ = command.args([
            "-NoProfile",
            "-Command",
            "Start-Sleep",
            "-Seconds",
            CHILD_LIFE,
        ]);
        command
    } else {
        let mut command = std::process::Command::new("sleep");
        let _ = command.arg(CHILD_LIFE);
        command
    }
}

/// A `memory/start` request for `pid` on the refusing runtime.
fn start_params(pid: u32) -> Value {
    json!({"pid": i64::from(pid), "runtime": RUNTIME_ID})
}

/// A service whose only runtime is `runtime`.
fn service(runtime: Arc<RefusingRuntime>) -> ProfilerService {
    let plugin: Arc<dyn Runtime> = runtime;
    let events: Arc<dyn ProfilerEvents> = Arc::new(Discard);
    let config = ProfilerConfig {
        output_dir: std::env::temp_dir().join(OUTPUT_DIR),
        ..ProfilerConfig::default()
    };
    ProfilerService::new(vec![plugin], config, events)
}

#[tokio::test]
async fn a_failed_memory_start_releases_the_session_it_opened() -> Result<(), String> {
    let tracked = child()?;
    let pid = tracked.0.id();
    let runtime = Arc::new(RefusingRuntime {
        pid,
        backend: Refusing::default(),
    });
    let service = service(Arc::clone(&runtime));
    let baseline = Arc::strong_count(&runtime);
    for _ in 0..ATTEMPTS {
        refuse_once(&service, pid).await?;
    }
    check_backend(&runtime)?;
    check_manager(&runtime, baseline)
}

/// One refused `memory/start` through the real dispatch path.
async fn refuse_once(service: &ProfilerService, pid: u32) -> Result<(), String> {
    let error = service
        .handle(methods::MEMORY_START, start_params(pid))
        .await
        .err()
        .ok_or("a refused backend start still answered")?;
    match error {
        ProfilerError::Memory(MemoryError::Backend(message)) if message == REFUSED => Ok(()),
        other => Err(format!("unexpected error for a refused start: {other}")),
    }
}

/// Every attempted start must have reached the backend's `stop`.
fn check_backend(runtime: &Arc<RefusingRuntime>) -> Result<(), String> {
    let started = runtime.backend.starts();
    let stopped = runtime.backend.stops();
    if started.len() != ATTEMPTS {
        return Err(format!(
            "{} of {ATTEMPTS} starts reached the backend",
            started.len()
        ));
    }
    if stopped != started {
        return Err(format!(
            "a failed start never stopped its session: {} stops for {} starts",
            stopped.len(),
            started.len()
        ));
    }
    Ok(())
}

/// Every open session holds a clone of its runtime, so while the service is
/// alive the runtime's strong count is back at its pre-start baseline only if
/// the manager kept no session from the failed starts.
fn check_manager(runtime: &Arc<RefusingRuntime>, baseline: usize) -> Result<(), String> {
    let retained = Arc::strong_count(runtime).saturating_sub(baseline);
    if retained != 0 {
        return Err(format!(
            "{retained} of {ATTEMPTS} failed starts left their sessions in the manager"
        ));
    }
    Ok(())
}
