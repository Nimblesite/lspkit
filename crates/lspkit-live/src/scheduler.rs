//! Debouncing single-flight scheduler.
//!
//! Coalesces rapid bursts of change events into a single re-analysis trigger.
//! Two windows control coalescing:
//!
//! - `quiet`: time without further events before firing.
//! - `cap`: maximum wait before firing regardless of activity.

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time;

use crate::watcher::ChangeEvent;

/// Scheduler configuration.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct ScheduleConfig {
    /// Wait this long without further events before firing.
    pub quiet: Duration,
    /// Fire after at most this much elapsed time since the first event.
    pub cap: Duration,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(100),
            cap: Duration::from_millis(500),
        }
    }
}

/// A coalesced trigger emitted by the scheduler.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Trigger {
    /// All events seen since the last fire.
    pub events: Vec<ChangeEvent>,
}

/// Run a debouncing single-flight scheduler.
///
/// Reads change events from `events_rx`, coalesces them per `config`, and
/// pushes one [`Trigger`] per quiet window to the returned receiver.
#[must_use]
pub fn spawn(
    mut events_rx: mpsc::UnboundedReceiver<ChangeEvent>,
    config: ScheduleConfig,
) -> mpsc::UnboundedReceiver<Trigger> {
    let (tx, rx) = mpsc::unbounded_channel();
    let _scheduler = tokio::spawn(async move {
        let mut buffered: Vec<ChangeEvent> = Vec::new();
        let mut first_event_at: Option<time::Instant> = None;

        loop {
            let next_event = if buffered.is_empty() {
                events_rx.recv().await
            } else {
                let deadline = compute_deadline(first_event_at, config);
                let Ok(event) = time::timeout_at(deadline, events_rx.recv()).await else {
                    let trigger = Trigger {
                        events: std::mem::take(&mut buffered),
                    };
                    first_event_at = None;
                    if tx.send(trigger).is_err() {
                        return;
                    }
                    continue;
                };
                event
            };

            let Some(event) = next_event else { return };
            if buffered.is_empty() {
                first_event_at = Some(time::Instant::now());
            }
            buffered.push(event);
        }
    });
    rx
}

/// Bounds the next quiet-window deadline by the burst's maximum wait.
fn compute_deadline(first: Option<time::Instant>, config: ScheduleConfig) -> time::Instant {
    let now = time::Instant::now();
    let quiet_deadline = now + config.quiet;
    match first {
        Some(start) => {
            let cap_deadline = start + config.cap;
            quiet_deadline.min(cap_deadline)
        }
        None => quiet_deadline,
    }
}
