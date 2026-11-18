//! The producer task belongs to its sampler handle. [PROF-CPU-SESSIONS]

use std::sync::Arc;
use std::time::Duration;

use super::SamplerHandle;

/// Bound on how long a stop waits; never reached here.
const FINISH: Duration = Duration::from_millis(10);

#[tokio::test]
async fn dropping_the_handle_ends_a_producer_that_never_finishes() {
    let mut parts = SamplerHandle::channel(42, String::new(), FINISH);
    let state = Arc::new(());
    let held = Arc::downgrade(&state);
    let producer = tokio::spawn(async move {
        std::future::pending::<()>().await;
        drop(state);
    });
    parts.handle.own_producer(&producer);
    tokio::task::yield_now().await;
    assert_eq!(held.strong_count(), 1, "the producer is running");
    drop(parts.handle);
    tokio::task::yield_now().await;
    assert_eq!(
        held.strong_count(),
        0,
        "the producer outlived its dropped sampler handle"
    );
}
