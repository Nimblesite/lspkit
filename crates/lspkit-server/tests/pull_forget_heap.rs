//! [LSP-PULL-FORGET] [MEM-FIX-SCOPE] Open/forget cycles of distinct URIs leave
//! no heap behind. Single-test binary: the instrumented allocator sees no
//! sibling tests.
#![cfg(feature = "pull")]

use std::sync::Arc;

use lspkit_server::pull::{PullError, PullStore};

#[path = "../../../test-support/state.rs"]
mod state;

/// Cycles that size every reusable buffer before measurement.
const WARM_CYCLES: usize = 1_000;
/// Measured cycles, each on a URI never seen before.
const CYCLES: usize = 1_000;

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
}
