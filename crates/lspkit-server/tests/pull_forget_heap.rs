//! [LSP-PULL-FORGET] [MEM-LIFETIME] Open/forget cycles of distinct URIs leave
//! no heap behind. Single-test binary: the instrumented allocator sees no
//! sibling tests.
#![cfg(feature = "pull")]

use std::sync::{Arc, Barrier};

use lspkit_server::pull::{PullError, PullStore};

#[path = "../../../test-support/state.rs"]
mod state;

/// Cycles that size every reusable buffer before measurement.
const WARM_CYCLES: usize = 1_000;
/// Measured cycles, each on a URI never seen before.
const CYCLES: usize = 1_000;

/// Documents left open, then closed while the store is idle.
const OPEN_DOCUMENTS: usize = 32;
/// Heap each open document's report owns.
const REPORT_BYTES: usize = 64 * 1024;
/// Closes of one URI with no pull in between.
const REPEATED_CLOSES: usize = 1_000;

/// A store whose every report owns `REPORT_BYTES` of heap.
fn heavy_store() -> PullStore {
    PullStore::new(Arc::new(|_uri| {
        Ok(Arc::new(vec![lsp_types::Diagnostic {
            message: "x".repeat(REPORT_BYTES),
            ..lsp_types::Diagnostic::default()
        }]))
    }))
}

/// The document whose analysis is held in flight.
const SLOW: &str = "file:///repo/slow.cs";

/// A heavy store whose analysis of `SLOW` waits on `started` then `release`.
fn held_store(started: &Arc<Barrier>, release: &Arc<Barrier>) -> PullStore {
    let (started, release) = (Arc::clone(started), Arc::clone(release));
    PullStore::new(Arc::new(move |uri| {
        if uri == SLOW {
            let _started = started.wait();
            let _released = release.wait();
        }
        Ok(Arc::new(vec![lsp_types::Diagnostic {
            message: "x".repeat(REPORT_BYTES),
            ..lsp_types::Diagnostic::default()
        }]))
    }))
}

/// The URI of open document `index`.
fn open_uri(index: usize) -> String {
    format!("file:///repo/open-{index:06}.cs")
}

/// Open, pull and close `count` distinct documents starting at `first`.
fn cycles(store: &PullStore, first: usize, count: usize) -> Result<(), PullError> {
    (first..first + count).try_for_each(|index| {
        // One width for every URI: Salsa frees a replaced memo one revision
        // late, so equal sizes keep that one-cycle lag byte-neutral.
        let uri = format!("file:///repo/doc-{index:06}.cs");
        let _report = store.diagnostics(&uri, 1)?;
        store.forget(&uri)
    })
}

#[test]
fn open_forget_cycles_of_distinct_uris_release_their_heap() {
    let store = PullStore::new(Arc::new(|uri| {
        Ok(Arc::new(vec![lsp_types::Diagnostic {
            message: uri.repeat(16),
            ..lsp_types::Diagnostic::default()
        }]))
    }));
    cycles(&store, 0, WARM_CYCLES).expect("warm-up cycles");
    let mut outcome = Ok(());
    state::assert_releases_heap(|| outcome = cycles(&store, WARM_CYCLES, CYCLES));
    outcome.expect("measured cycles");

    // Close every tab and go idle: the reports are freed by the closes
    // themselves, not by some later pull.
    let heavy = heavy_store();
    for index in 0..OPEN_DOCUMENTS {
        let _open = heavy
            .diagnostics(&open_uri(index), 1)
            .expect("an open pull");
    }
    let uris: Vec<String> = (0..OPEN_DOCUMENTS).map(open_uri).collect();
    let mut closed = Ok(());
    let stats = state::heap_change(|| {
        closed = uris.iter().try_for_each(|uri| heavy.forget(uri));
    });
    closed.expect("idle closes");
    assert!(
        stats.bytes_deallocated >= OPEN_DOCUMENTS * REPORT_BYTES,
        "closing {OPEN_DOCUMENTS} idle documents freed {} of their {} report bytes",
        stats.bytes_deallocated,
        OPEN_DOCUMENTS * REPORT_BYTES
    );

    // A close that arrives while a pull holds the state is retired by that
    // pull as it finishes, with no later pull.
    let (started, release) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
    let held = held_store(&started, &release);
    // The caller drops its copy, so only the memo still owns the report.
    drop(held.diagnostics(&open_uri(0), 1).expect("an open pull"));
    let worker = {
        let held = held.clone();
        std::thread::spawn(move || held.diagnostics(SLOW, 1))
    };
    let _in_flight = started.wait();
    held.forget(&open_uri(0)).expect("a close during a pull");
    let stats = state::heap_change(|| {
        let _release = release.wait();
        let _slow = worker.join();
    });
    assert!(
        stats.bytes_deallocated >= REPORT_BYTES,
        "a close during a pull freed {} of its {REPORT_BYTES} report bytes",
        stats.bytes_deallocated
    );

    // Repeated closes with no pull in between keep nothing queued.
    let mut repeated = Ok(());
    state::assert_releases_heap(|| {
        repeated = (0..REPEATED_CLOSES).try_for_each(|_| heavy.forget(&open_uri(0)));
    });
    repeated.expect("repeated closes");
}
