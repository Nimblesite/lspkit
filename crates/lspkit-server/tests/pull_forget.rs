//! [LSP-PULL-FORGET] Closing one document releases only its memos: every other
//! open document stays warm, and the closed one recomputes when reopened.
#![cfg(feature = "pull")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lsp_types::Diagnostic;
use lspkit_server::pull::PullStore;

/// Compiler calls per URI.
type Calls = Arc<Mutex<HashMap<String, usize>>>;

const A: &str = "file:///repo/a.cs";
const B: &str = "file:///repo/b.cs";
const C: &str = "file:///repo/c.cs";
const D: &str = "file:///repo/d.cs";

/// A store whose compiler reports one diagnostic naming the URI it analyzed.
fn counting_store() -> (PullStore, Calls) {
    let calls: Calls = Arc::default();
    let counted = Arc::clone(&calls);
    let store = PullStore::new(Arc::new(move |uri| {
        if let Ok(mut calls) = counted.lock() {
            *calls.entry(uri.to_owned()).or_default() += 1;
        }
        let named = Diagnostic {
            message: uri.to_owned(),
            ..Diagnostic::default()
        };
        Ok(Arc::new(vec![named]))
    }));
    (store, calls)
}

/// How many times the compiler ran, over every URI.
fn total_calls(calls: &Calls) -> usize {
    match calls.lock() {
        Ok(counts) => counts.values().sum(),
        Err(poisoned) => poisoned.into_inner().values().sum(),
    }
}

/// How many times the compiler analyzed `uri`.
fn calls_for(calls: &Calls, uri: &str) -> usize {
    let counts = match calls.lock() {
        Ok(counts) => counts,
        Err(poisoned) => poisoned.into_inner(),
    };
    match counts.get(uri) {
        Some(count) => *count,
        None => 0,
    }
}

/// Pull `uri` and assert the report names it, never another document.
fn pull(store: &PullStore, uri: &str) -> String {
    let pulled = store.diagnostics(uri, 1);
    assert!(pulled.is_ok(), "{uri}: {pulled:?}");
    let Ok(report) = pulled else {
        return String::new();
    };
    let messages: Vec<&str> = report
        .items
        .iter()
        .map(|item| item.message.as_str())
        .collect();
    assert_eq!(messages, [uri], "{uri} reports only its own diagnostics");
    report.result_id
}

#[test]
fn forgetting_one_document_keeps_every_other_memo_warm() {
    let (store, calls) = counting_store();
    let [_id_a, id_b, id_c] = [A, B, C].map(|uri| pull(&store, uri));
    for uri in [A, B, C] {
        let _warm = pull(&store, uri);
        assert_eq!(calls_for(&calls, uri), 1, "{uri} is memoized");
    }
    let revision = store.revision();
    let before = total_calls(&calls);
    assert_eq!(store.forget(A), Ok(()), "forget a closed document");
    assert_eq!(store.revision(), revision, "forgetting supersedes nothing");
    assert_eq!(
        total_calls(&calls),
        before,
        "a close never runs the compiler"
    );
    assert_eq!(calls_for(&calls, ""), 0, "a retired slot is never analyzed");
    for (uri, id) in [(B, id_b), (C, id_c)] {
        assert_eq!(pull(&store, uri), id, "{uri} keeps its snapshot");
        assert_eq!(calls_for(&calls, uri), 1, "{uri} stays a cache hit");
    }
    let _reopened = pull(&store, A);
    assert_eq!(calls_for(&calls, A), 2, "a forgotten document recomputes");
    let _memo = pull(&store, A);
    assert_eq!(calls_for(&calls, A), 2, "and is memoized again");
}

#[test]
fn forget_is_idempotent_and_a_recycled_input_never_replays_old_truth() {
    let (store, calls) = counting_store();
    for uri in [A, B, C] {
        let _first = pull(&store, uri);
    }
    assert_eq!(
        store.forget("file:///never/opened.cs"),
        Ok(()),
        "an unknown URI"
    );
    assert_eq!(store.forget(B), Ok(()), "first forget");
    assert_eq!(store.forget(B), Ok(()), "a repeated forget");
    let _fresh = pull(&store, D);
    assert_eq!(
        calls_for(&calls, D),
        1,
        "a new document computes its own truth"
    );
    let _back = pull(&store, B);
    assert_eq!(calls_for(&calls, B), 2, "B recomputes after its forget");
    for uri in [A, C] {
        let _warm = pull(&store, uri);
        assert_eq!(calls_for(&calls, uri), 1, "{uri} never recomputed");
    }
    assert_eq!(store.clear(), Ok(()), "clear still drops everything");
    let _cold = pull(&store, C);
    assert_eq!(calls_for(&calls, C), 2, "clear recomputes every document");
}

#[test]
fn forget_frees_the_closed_documents_diagnostics_before_it_returns() {
    let (store, _calls) = counting_store();
    let held = |uri| {
        let pulled = store.diagnostics(uri, 1);
        assert!(pulled.is_ok(), "{uri}: {pulled:?}");
        pulled.map(|report| Arc::downgrade(&report.items)).ok()
    };
    let (a, b) = (held(A), held(B));
    let strong = |weak: &Option<std::sync::Weak<Vec<Diagnostic>>>| {
        weak.as_ref().map(std::sync::Weak::strong_count)
    };
    assert_eq!(
        (strong(&a), strong(&b)),
        (Some(1), Some(1)),
        "only the memos hold them"
    );
    assert_eq!(store.forget(A), Ok(()), "forget A");
    assert_eq!(
        strong(&a),
        Some(0),
        "A's diagnostics are freed with no later pull"
    );
    assert_eq!(strong(&b), Some(1), "B's memo is untouched");
}

#[test]
fn forget_returns_while_a_compiler_query_runs_and_the_next_pull_applies_it() {
    let started = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let calls: Calls = Arc::default();
    let (in_flight, resume, counted) = (
        Arc::clone(&started),
        Arc::clone(&release),
        Arc::clone(&calls),
    );
    let store = PullStore::new(Arc::new(move |uri| {
        let seen = counted.lock().map(|mut calls| {
            let count = calls.entry(uri.to_owned()).or_default();
            *count += 1;
            *count
        });
        if uri == B && matches!(seen, Ok(1)) {
            let _started = in_flight.wait();
            let _released = resume.wait();
        }
        Ok(Arc::new(vec![Diagnostic {
            message: uri.to_owned(),
            ..Diagnostic::default()
        }]))
    }));
    let _a = pull(&store, A);
    let worker = {
        let store = store.clone();
        std::thread::spawn(move || store.diagnostics(B, 1).map(|report| report.result_id))
    };
    let _computing = started.wait();
    assert_eq!(
        store.forget(A),
        Ok(()),
        "forget never waits for B's compiler"
    );
    let _done = release.wait();
    assert!(matches!(worker.join(), Ok(Ok(_))), "B completes");
    let _reopened = pull(&store, A);
    assert_eq!(
        calls_for(&calls, A),
        2,
        "the queued forget applied before A's pull"
    );
}

#[test]
fn an_empty_uri_is_analyzed_never_mistaken_for_a_forgotten_slot() {
    let (store, calls) = counting_store();
    let _empty = pull(&store, "");
    assert_eq!(
        calls_for(&calls, ""),
        1,
        "the compiler decides what \"\" means"
    );
}
