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
    store.forget(A).expect("forget a closed document");
    assert_eq!(store.revision(), revision, "forgetting supersedes nothing");
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
    store
        .forget("file:///never/opened.cs")
        .expect("an unknown URI");
    store.forget(B).expect("first forget");
    store.forget(B).expect("a repeated forget");
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
    store.clear().expect("clear still drops everything");
    let _cold = pull(&store, C);
    assert_eq!(calls_for(&calls, C), 2, "clear recomputes every document");
}
