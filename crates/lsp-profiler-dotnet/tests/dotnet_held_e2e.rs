//! Memory steps their target holds, end to end. Tests [PROF-MEMORY-STOP].
//!
//! A frozen target never answers the engine, so a snapshot or a diff of it
//! waits for as long as the target stays frozen: the kernel's own stop state
//! (`kill -STOP`, confirmed by `ps`) holds the step, never a timer. Stopping
//! the session must still answer, release every step it holds and leave the
//! target alive; stopping one session must leave another's steps alone. The
//! guard only bounds a regression, which would otherwise hang.

#![cfg(unix)]
#![allow(
    clippy::allow_attributes,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    missing_docs
)]

#[path = "../../../test-support/stopped_process.rs"]
mod stopped_process;
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lsp_profiler::wire::methods;
use lsp_profiler::{ProfilerError, ProfilerService};
use serde_json::{json, Value};
use support::{call, completed, service, Fixture};
use tokio::task::JoinHandle;

const FIXTURE: &str = "HotLeakApp";
/// Bounds a regression: a released step answers at once, a held one never does.
const GUARD: Duration = Duration::from_secs(60);
/// The code every memory failure that is not an attach failure answers with ([PROF-ERRORS]).
const OTHER: i64 = -32000;

type Step = JoinHandle<Result<Value, ProfilerError>>;

/// A tracked fixture frozen in the kernel's stop state, and its session.
struct Held {
    fixture: Fixture,
    frozen: stopped_process::Stopped,
    id: String,
}

impl Held {
    /// Track a fresh fixture, then freeze it.
    async fn start(service: &ProfilerService) -> Self {
        let fixture = Fixture::start(FIXTURE);
        let started = call(
            service,
            methods::MEMORY_START,
            json!({ "pid": fixture.pid }),
        )
        .await;
        let id = started["memory_session_id"]
            .as_str()
            .expect("session id")
            .to_owned();
        let frozen = stopped_process::suspend(fixture.pid, GUARD).await;
        Self {
            fixture,
            frozen,
            id,
        }
    }

    /// Let the target run again.
    fn thaw(self) -> Fixture {
        self.frozen.resume();
        self.fixture
    }

    fn session(&self) -> Value {
        json!({ "memory_session_id": self.id })
    }

    /// Where the session's graphs live ([PROF-DOTNET-MEMORY]).
    fn graphs(&self, dir: &Path) -> PathBuf {
        dir.join(format!("heap-{}", self.id))
    }

    /// Start `method`; settles once the step has made the session's graph
    /// directory, so it is past its session check and waits on the target.
    async fn in_flight(
        &self,
        service: &Arc<ProfilerService>,
        dir: &Path,
        method: &'static str,
    ) -> Step {
        let (service, session) = (Arc::clone(service), self.session());
        let step = tokio::spawn(async move { service.handle(method, session).await });
        let graphs = self.graphs(dir);
        tokio::time::timeout(GUARD, async {
            while !graphs.exists() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the step reaches its frozen target");
        step
    }

    /// The target outlived everything the profiler did to it.
    fn assert_alive(&mut self) {
        assert!(
            self.fixture
                .child
                .try_wait()
                .expect("read the target's state")
                .is_none(),
            "stopping a session never ends its target"
        );
    }
}

/// The step's answer, which a released step gives at once.
async fn answer(step: Step) -> Result<Value, ProfilerError> {
    tokio::time::timeout(GUARD, step)
        .await
        .expect("the released step answers")
        .expect("the step's task completes")
}

/// A step stop released: refused as stopped, naming its session.
fn assert_released(answer: Result<Value, ProfilerError>, id: &str) {
    let error = answer.expect_err("a released step is refused, never completed");
    assert_eq!(error.code(), OTHER, "{error}");
    assert_eq!(
        error.to_string(),
        format!("memory session {id} stopped before the step finished")
    );
}

/// Stop answers `Ack`; afterwards the session's graphs are gone and every step is refused.
async fn assert_stopped(service: &ProfilerService, dir: &Path, held: &Held) {
    let stopped = call(service, methods::MEMORY_STOP, held.session()).await;
    assert_eq!(completed(&stopped)["outcome"]["kind"], "Ack", "{stopped}");
    assert!(
        !held.graphs(dir).exists(),
        "stop deletes the session's graphs"
    );
    let later = service
        .handle(methods::MEMORY_SNAPSHOT, held.session())
        .await;
    let error = later.expect_err("a stopped session refuses every step");
    assert_eq!(error.code(), OTHER, "{error}");
    assert_eq!(
        error.to_string(),
        format!("unknown memory session: {}", held.id)
    );
}

/// The thawed target is whole: a new session captures its heap at once.
async fn assert_capturable(service: &ProfilerService, target: &Fixture) {
    let started = call(service, methods::MEMORY_START, json!({ "pid": target.pid })).await;
    let session = json!({ "memory_session_id": started["memory_session_id"] });
    let snapshot = call(service, methods::MEMORY_SNAPSHOT, session.clone()).await;
    assert_eq!(
        completed(&snapshot)["outcome"]["kind"],
        "Snapshot",
        "{snapshot}"
    );
    let stopped = call(service, methods::MEMORY_STOP, session).await;
    assert_eq!(completed(&stopped)["outcome"]["kind"], "Ack", "{stopped}");
}

async fn stop_releases(method: &'static str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = Arc::new(service(dir.path().to_path_buf()));
    let mut held = Held::start(&service).await;
    let step = held.in_flight(&service, dir.path(), method).await;
    assert!(!step.is_finished(), "a frozen target holds the {method}");

    assert_stopped(&service, dir.path(), &held).await;

    assert_released(answer(step).await, &held.id);
    held.assert_alive();
    assert_capturable(&service, &held.thaw()).await;
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_releases_a_snapshot_its_frozen_target_holds() {
    stop_releases(methods::MEMORY_SNAPSHOT).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_releases_a_diff_its_frozen_target_holds() {
    stop_releases(methods::MEMORY_DIFF).await;
}

/// Each session's stop releases its own steps only.
#[tokio::test(flavor = "multi_thread")]
async fn stopping_one_session_leaves_another_sessions_held_step_held() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = Arc::new(service(dir.path().to_path_buf()));
    let mut first = Held::start(&service).await;
    let mut second = Held::start(&service).await;
    let first_step = first
        .in_flight(&service, dir.path(), methods::MEMORY_SNAPSHOT)
        .await;
    let second_step = second
        .in_flight(&service, dir.path(), methods::MEMORY_DIFF)
        .await;

    assert_stopped(&service, dir.path(), &first).await;
    assert_released(answer(first_step).await, &first.id);
    assert!(
        !second_step.is_finished(),
        "the other session's frozen target still holds its step"
    );
    assert!(
        second.graphs(dir.path()).exists(),
        "the other session keeps its graphs"
    );

    assert_stopped(&service, dir.path(), &second).await;
    assert_released(answer(second_step).await, &second.id);
    first.assert_alive();
    second.assert_alive();
    assert_capturable(&service, &first.thaw()).await;
    assert_capturable(&service, &second.thaw()).await;
    service.shutdown().await;
}
