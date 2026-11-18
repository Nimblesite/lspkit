//! Live CPU progress notifications ([PROF-CPU-PROGRESS]).
//!
//! [PROF-CPU-PROGRESS] The task holds only a weak back-reference, upgraded
//! per tick, so it never keeps a dropped service (and its transport) alive.

use std::sync::Weak;
use std::time::Duration;

use tracing::{debug, warn};

use super::Inner;
use crate::wire::methods;

/// How often a live session publishes progress.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);

/// Spawn the notifier for one session. It ends when the session leaves the
/// manager or the service is dropped.
pub(super) fn spawn(service: Weak<Inner>, session_id: String) {
    let _task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let _tick = ticker.tick().await;
            let Some(inner) = service.upgrade() else {
                debug!(%session_id, "progress notifier outlived its service");
                return;
            };
            if !publish(&inner, &session_id).await {
                return;
            }
        }
    });
}

/// Publish one progress notification; `false` once the session is gone.
async fn publish(inner: &Inner, session_id: &str) -> bool {
    let Some(progress) = inner.cpu.progress(session_id, &inner.config.hotspots).await else {
        debug!(%session_id, "progress notifier finished");
        return false;
    };
    match serde_json::to_value(&progress) {
        Ok(params) => inner.events.notify(methods::CPU_PROGRESS, params).await,
        Err(error) => warn!(%error, "cannot encode progress"),
    }
    true
}
