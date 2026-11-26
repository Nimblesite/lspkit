//! E2E scheduler tests. Drive virtual time via `tokio::time::pause()`.
//!
//! `ScheduleConfig` is `#[non_exhaustive]`, so we use `Default::default()`:
//! quiet = 100ms, cap = 500ms.

#![deny(missing_docs)]

use std::path::PathBuf;
use std::time::Duration;

use lspkit_server::live::{
    spawn_scheduler, ChangeEvent, ScheduleConfig, Trigger, MAX_PENDING_EVENTS,
};
use tokio::sync::mpsc;
use tokio::time;

const QUIET: Duration = Duration::from_millis(100);
const CAP: Duration = Duration::from_millis(500);

/// Build a fresh scheduler with default config.
fn build() -> (mpsc::UnboundedSender<ChangeEvent>, mpsc::Receiver<Trigger>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let triggers = spawn_scheduler(rx, ScheduleConfig::default());
    (tx, triggers)
}

/// Yield repeatedly so the spawned scheduler task observes pending work.
async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

/// Send `event`, then drive virtual time past the quiet window and settle.
async fn send_and_quiet(tx: &mpsc::UnboundedSender<ChangeEvent>, event: ChangeEvent) -> Option<()> {
    tx.send(event).ok()?;
    settle().await;
    time::advance(QUIET + Duration::from_millis(20)).await;
    settle().await;
    Some(())
}

/// Convenience: build a `Touched` event for `name`.
fn touched(name: &str) -> ChangeEvent {
    ChangeEvent::Touched(vec![PathBuf::from(name)])
}

/// Convenience: build a `Removed` event for `name`.
fn removed(name: &str) -> ChangeEvent {
    ChangeEvent::Removed(vec![PathBuf::from(name)])
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn config_default_is_sane() {
    let cfg = ScheduleConfig::default();
    assert!(cfg.quiet < cfg.cap);
    assert!(cfg.quiet > Duration::ZERO);
    let copy = cfg;
    assert_eq!(copy.quiet, cfg.quiet);
    assert_eq!(copy.cap, cfg.cap);
    assert!(format!("{cfg:?}").contains("ScheduleConfig"));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn single_event_fires_after_quiet_window() {
    let (tx, mut triggers) = build();

    send_and_quiet(&tx, touched("a.rs"))
        .await
        .expect("send first event");

    let trig = triggers.recv().await.expect("trigger");
    assert_eq!(trig.events.len(), 1);
    let got = match trig.events.first().expect("single scheduled event") {
        ChangeEvent::Touched(p) => p.clone(),
        _ => Vec::new(),
    };
    assert_eq!(got, vec![PathBuf::from("a.rs")]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn burst_is_coalesced_into_one_trigger() {
    let (tx, mut triggers) = build();

    // Send 5 events spaced < QUIET apart so the quiet window keeps resetting.
    for idx in 0..5 {
        tx.send(ChangeEvent::Touched(vec![PathBuf::from(format!(
            "f{idx}.rs"
        ))]))
        .expect("send");
        settle().await;
        time::advance(Duration::from_millis(20)).await;
        settle().await;
    }
    // Now wait out the quiet window.
    time::advance(QUIET + Duration::from_millis(20)).await;
    settle().await;

    let trig = triggers.recv().await.expect("trigger");
    assert_eq!(trig.events.len(), 5);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cap_fires_even_under_sustained_activity() {
    let (tx, mut triggers) = build();

    // Send events at 50ms intervals (< QUIET=100ms) for longer than CAP=500ms.
    // The cap should force a fire before quiet ever elapses.
    for _ in 0..15 {
        tx.send(ChangeEvent::Removed(vec![PathBuf::from("x")]))
            .expect("send");
        settle().await;
        time::advance(Duration::from_millis(50)).await;
        settle().await;
    }

    let trig = triggers.recv().await.expect("trigger");
    assert!(!trig.events.is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn closing_input_after_fire_closes_trigger_channel() {
    let (tx, mut triggers) = build();

    send_and_quiet(&tx, touched("z.rs"))
        .await
        .expect("send event before close");

    let first = triggers.recv().await.expect("first");
    assert_eq!(first.events.len(), 1);

    drop(tx);
    settle().await;
    let next = triggers.recv().await;
    assert!(next.is_none());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn closing_input_with_no_events_closes_trigger_channel() {
    let (tx, mut triggers) = build();
    drop(tx);
    settle().await;
    let next = triggers.recv().await;
    assert!(next.is_none());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn trigger_is_clone_and_debug() {
    let (tx, mut triggers) = build();
    send_and_quiet(&tx, touched("a"))
        .await
        .expect("send cloned trigger event");
    let trig = triggers.recv().await.expect("trigger");
    let cloned = trig.clone();
    assert_eq!(cloned.events.len(), trig.events.len());
    assert!(format!("{trig:?}").contains("Trigger"));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn dropping_trigger_receiver_terminates_task_gracefully() {
    let (tx, triggers) = build();
    tx.send(ChangeEvent::Touched(vec![PathBuf::from("a")]))
        .expect("send");
    settle().await;
    drop(triggers);
    time::advance(QUIET + Duration::from_millis(20)).await;
    settle().await;
    // Sending more should not panic the task (channel send may or may not succeed).
    let _ = tx.send(ChangeEvent::Removed(vec![PathBuf::from("b")]));
    settle().await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn second_burst_after_first_fire_yields_second_trigger() {
    let (tx, mut triggers) = build();

    send_and_quiet(&tx, touched("a"))
        .await
        .expect("send first burst");
    let first = triggers.recv().await.expect("first");
    assert_eq!(first.events.len(), 1);

    send_and_quiet(&tx, removed("b"))
        .await
        .expect("send second burst");
    let second = triggers.recv().await.expect("second");
    assert_eq!(second.events.len(), 1);

    // Total observed: two independent triggers, each with one event.
    assert!(matches!(
        second.events.first().expect("second burst event"),
        ChangeEvent::Removed(_)
    ));

    // Sanity: the cap bound is non-zero.
    assert!(CAP > QUIET);
}

/// [MEM-LIFETIME-SCHEDULER-TASK] The scheduler task belongs to its consumer: once
/// the trigger receiver is dropped it must exit, releasing the event
/// receiver, even while no event ever arrives.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_scheduler_exits_when_its_consumer_drops_the_triggers() {
    let (tx, triggers) = build();
    settle().await;
    assert!(!tx.is_closed(), "the scheduler is running");
    drop(triggers);
    settle().await;
    assert!(
        tx.is_closed(),
        "the scheduler task outlived its consumer and still holds the event receiver"
    );
}

/// Paths of every path-carrying event in `trigger`, in order.
fn paths_of(trigger: &Trigger) -> Vec<PathBuf> {
    trigger
        .events
        .iter()
        .flat_map(|event| match event {
            ChangeEvent::Touched(paths) | ChangeEvent::Removed(paths) => paths.clone(),
            _ => Vec::new(),
        })
        .collect()
}

/// [MEM-LIFETIME-SCHEDULER-BACKLOG] While the consumer is still analysing, later
/// bursts merge into one waiting trigger instead of queueing one trigger each,
/// and the waiting trigger is delivered as soon as the consumer reads again.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn bursts_behind_a_busy_consumer_merge_into_one_waiting_trigger() {
    let (tx, mut triggers) = build();
    for name in ["a.rs", "b.rs", "c.rs", "d.rs"] {
        send_and_quiet(&tx, touched(name)).await.expect("send");
    }
    let first = triggers.recv().await.expect("first trigger");
    assert_eq!(paths_of(&first), vec![PathBuf::from("a.rs")]);
    settle().await;
    let merged = triggers.recv().await.expect("merged trigger");
    assert_eq!(
        paths_of(&merged),
        ["b.rs", "c.rs", "d.rs"].map(PathBuf::from).to_vec(),
        "every change made while the consumer was busy arrives, in order, in one trigger"
    );
    settle().await;
    assert!(
        triggers.try_recv().is_err(),
        "a busy consumer must never have more than one trigger waiting"
    );
    send_and_quiet(&tx, removed("e.rs")).await.expect("send");
    let after = triggers.recv().await.expect("trigger after catching up");
    assert_eq!(paths_of(&after), vec![PathBuf::from("e.rs")]);
    assert!(matches!(after.events.as_slice(), [ChangeEvent::Removed(_)]));
}

/// [MEM-LIFETIME-SCHEDULER-BACKLOG] A consumer that stays busy through endless churn
/// holds at most `MAX_PENDING_EVENTS` waiting events: past that the waiting
/// trigger becomes one `Rescan`, which covers every change it replaced.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_backlog_past_the_bound_collapses_to_one_rescan() {
    let (tx, mut triggers) = build();
    send_and_quiet(&tx, touched("first.rs"))
        .await
        .expect("send");
    send_and_quiet(&tx, touched("waiting.rs"))
        .await
        .expect("send");
    for index in 0..MAX_PENDING_EVENTS + 10 {
        tx.send(touched(&format!("churn{index}.rs"))).expect("send");
    }
    settle().await;
    let first = triggers.recv().await.expect("first trigger");
    assert_eq!(paths_of(&first), vec![PathBuf::from("first.rs")]);
    settle().await;
    let collapsed = triggers.recv().await.expect("collapsed trigger");
    assert!(
        matches!(collapsed.events.as_slice(), [ChangeEvent::Rescan]),
        "an overflowing backlog is exactly one Rescan, got {} events",
        collapsed.events.len()
    );
    settle().await;
    assert!(triggers.try_recv().is_err(), "nothing else may be queued");
    send_and_quiet(&tx, touched("fresh.rs"))
        .await
        .expect("send");
    let fresh = triggers.recv().await.expect("trigger after the rescan");
    assert_eq!(paths_of(&fresh), vec![PathBuf::from("fresh.rs")]);
}

/// [MEM-LIFETIME-SCHEDULER-BACKLOG] A burst whose window has elapsed is owed to
/// the consumer even when the events end while the consumer is still busy: the
/// scheduler delivers it before closing, as it does for a free consumer.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn events_ending_behind_a_busy_consumer_still_deliver_the_ready_burst() {
    let (tx, mut triggers) = build();
    send_and_quiet(&tx, touched("first.rs"))
        .await
        .expect("send the burst the consumer is still holding");
    send_and_quiet(&tx, removed("second.rs"))
        .await
        .expect("send the burst that becomes ready behind it");
    drop(tx);
    settle().await;

    let first = triggers.recv().await.expect("the held trigger");
    assert!(
        matches!(first.events.as_slice(), [ChangeEvent::Touched(p)] if p == &vec![PathBuf::from("first.rs")])
    );
    let second = triggers
        .recv()
        .await
        .expect("the ready burst survives the end of the events");
    assert!(
        matches!(second.events.as_slice(), [ChangeEvent::Removed(p)] if p == &vec![PathBuf::from("second.rs")])
    );
    assert!(
        triggers.recv().await.is_none(),
        "the channel closes after the last burst"
    );
}

/// [MEM-LIFETIME-SCHEDULER-BACKLOG] Events that arrive after a burst is ready,
/// while the consumer is still busy, join that burst and are delivered with it
/// when the events end.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn events_absorbed_by_a_ready_burst_are_delivered_when_the_events_end() {
    let (tx, mut triggers) = build();
    send_and_quiet(&tx, touched("first.rs"))
        .await
        .expect("send the burst the consumer is still holding");
    send_and_quiet(&tx, touched("second.rs"))
        .await
        .expect("send the burst that becomes ready behind it");
    tx.send(removed("late.rs"))
        .expect("send an event into the ready burst");
    drop(tx);
    settle().await;

    let first = triggers.recv().await.expect("the held trigger");
    assert_eq!(paths_of(&first), vec![PathBuf::from("first.rs")]);
    let second = triggers
        .recv()
        .await
        .expect("the ready burst with the late event");
    assert_eq!(
        paths_of(&second),
        vec![PathBuf::from("second.rs"), PathBuf::from("late.rs")]
    );
    assert!(
        triggers.recv().await.is_none(),
        "the channel closes after the last burst"
    );
}
