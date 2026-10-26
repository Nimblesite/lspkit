//! The pull store's `Debug` output tracks its live snapshot revision and never exposes
//! analysis state. [LSP-PULL-SALSA]
#![cfg(feature = "pull")]

use std::sync::Arc;

use lspkit_server::pull::PullStore;

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
