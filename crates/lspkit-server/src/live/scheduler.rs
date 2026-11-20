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

use crate::live::watcher::ChangeEvent;

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
/// pushes one [`Trigger`] per quiet window to the returned receiver. The task
/// ends when the events end or the receiver is dropped.
#[must_use]
pub fn spawn(
    events_rx: mpsc::UnboundedReceiver<ChangeEvent>,
    config: ScheduleConfig,
) -> mpsc::UnboundedReceiver<Trigger> {
    let (tx, rx) = mpsc::unbounded_channel();
    let _scheduler = tokio::spawn(run(events_rx, tx, config));
    rx
}

/// What the scheduler observed next.
enum Step {
    /// A change event arrived.
    Event(ChangeEvent),
    /// The burst's window elapsed.
    Quiet,
    /// The event source closed.
    Ended,
}

/// Events seen since the last fire.
#[derive(Default)]
struct Burst {
    /// Buffered events, oldest first.
    events: Vec<ChangeEvent>,
    /// When the burst's first event arrived.
    first_event_at: Option<time::Instant>,
}

impl Burst {
    /// Wait for the next event, or for the window of a buffered burst.
    async fn next(
        &self,
        events_rx: &mut mpsc::UnboundedReceiver<ChangeEvent>,
        config: ScheduleConfig,
    ) -> Step {
        let event = if self.events.is_empty() {
            events_rx.recv().await
        } else {
            let deadline = compute_deadline(self.first_event_at, config);
            let Ok(event) = time::timeout_at(deadline, events_rx.recv()).await else {
                return Step::Quiet;
            };
            event
        };
        event.map_or(Step::Ended, Step::Event)
    }

    /// Buffer one event, starting the burst's cap window on the first.
    fn push(&mut self, event: ChangeEvent) {
        if self.events.is_empty() {
            self.first_event_at = Some(time::Instant::now());
        }
        self.events.push(event);
    }

    /// End the burst as one trigger.
    fn take(&mut self) -> Trigger {
        self.first_event_at = None;
        Trigger {
            events: std::mem::take(&mut self.events),
        }
    }
}

/// Coalesce events until the source ends or the consumer drops the triggers;
/// the task never outlives its consumer. [MEM-LIFETIME-SCHEDULER-TASK]
async fn run(
    mut events_rx: mpsc::UnboundedReceiver<ChangeEvent>,
    tx: mpsc::UnboundedSender<Trigger>,
    config: ScheduleConfig,
) {
    let mut burst = Burst::default();
    loop {
        let step = tokio::select! {
            biased;
            () = tx.closed() => return,
            step = burst.next(&mut events_rx, config) => step,
        };
        match step {
            Step::Event(event) => burst.push(event),
            Step::Quiet if tx.send(burst.take()).is_ok() => {}
            Step::Quiet | Step::Ended => return,
        }
    }
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
