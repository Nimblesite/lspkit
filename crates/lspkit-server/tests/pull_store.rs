//! The pull store's `Debug` output tracks its live snapshot revision and never exposes
//! analysis state. [LSP-PULL-SALSA]
#![cfg(feature = "pull")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread::JoinHandle;

use lspkit_server::pull::{PullError, PullReport, PullStore};

/// An analyzer whose first invocation is deterministically held in flight.
struct BlockedPull {
    /// Store under test.
    store: PullStore,
    /// Signals that the first analyzer invocation has started.
    started: Arc<Barrier>,
    /// Releases the first analyzer invocation.
    release: Arc<Barrier>,
    /// Number of actual compiler invocations.
    calls: Arc<AtomicUsize>,
}

impl BlockedPull {
    /// Block only the first call so the follow-up pull can verify recomputation.
    fn new() -> Self {
        let started = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let calls = Arc::new(AtomicUsize::new(0));
        let store = blocked_store(&started, &release, &calls);
        Self {
            store,
            started,
            release,
            calls,
        }
    }

    /// Capture the current identity and start one document pull on a worker.
    fn start(&self) -> (String, u64, JoinHandle<Result<PullReport, PullError>>) {
        let before = self.store.result_id(3);
        let revision = self.store.revision();
        let store = self.store.clone();
        let worker = std::thread::spawn(move || store.diagnostics("file:///source.cs", 3));
        (before, revision, worker)
    }
}

/// Construct a store whose first analyzer call waits on the supplied barriers.
fn blocked_store(
    started: &Arc<Barrier>,
    release: &Arc<Barrier>,
    calls: &Arc<AtomicUsize>,
) -> PullStore {
    let analyzer_started = Arc::clone(started);
    let analyzer_release = Arc::clone(release);
    let analyzer_calls = Arc::clone(calls);
    PullStore::new(Arc::new(move |_uri| {
        if analyzer_calls.fetch_add(1, Ordering::AcqRel) == 0 {
            let _started = analyzer_started.wait();
            let _released = analyzer_release.wait();
        }
        Ok(Arc::new(Vec::new()))
    }))
}

/// Require successful completion under the identity captured before the engine change.
fn assert_completed(worker: JoinHandle<Result<PullReport, PullError>>, expected: &str) {
    let result = worker.join();
    assert!(matches!(result, Ok(Ok(_))));
    if let Ok(Ok(report)) = result {
        assert_eq!(report.result_id, expected);
    }
}

#[test]
fn engine_change_preserves_in_flight_pull_and_advances_result_identity() {
    // An engine generation is not editor supersession. [LSP-PULL-CONCURRENCY]
    let probe = BlockedPull::new();
    let (before, revision, worker) = probe.start();
    let _started = probe.started.wait();
    probe.store.engine_changed();
    let _released = probe.release.wait();
    assert_completed(worker, &before);
    assert_eq!(probe.store.revision(), revision);
    let after = probe.store.result_id(3);
    assert_ne!(after, before);
    assert_eq!(
        probe
            .store
            .diagnostics("file:///source.cs", 3)
            .map(|report| report.result_id),
        Ok(after)
    );
    assert_eq!(probe.calls.load(Ordering::Acquire), 2);
}

#[test]
fn input_invalidation_still_supersedes_in_flight_pull() {
    // Editor inputs retain supersession semantics. [LSP-PULL-CONCURRENCY]
    let probe = BlockedPull::new();
    let (before, revision, worker) = probe.start();
    let _started = probe.started.wait();
    probe.store.invalidate();
    let _released = probe.release.wait();
    let result = worker.join();
    assert!(matches!(result, Ok(Err(PullError::Superseded))));
    assert_eq!(probe.store.revision(), revision + 1);
    assert_ne!(probe.store.result_id(3), before);
    assert_eq!(probe.calls.load(Ordering::Acquire), 1);
}

#[test]
fn debug_reports_the_current_revision_and_hides_analysis_state() {
    let store = PullStore::new(Arc::new(|_uri: &str| Ok(Arc::new(Vec::new()))));
    let initial = store.revision();
    assert_eq!(
        format!("{store:?}"),
        format!("PullStore {{ revision: {initial}, .. }}")
    );
    store.invalidate();
    let advanced = initial + 1;
    assert_eq!(store.revision(), advanced);
    assert_eq!(
        format!("{store:?}"),
        format!("PullStore {{ revision: {advanced}, .. }}")
    );
}
