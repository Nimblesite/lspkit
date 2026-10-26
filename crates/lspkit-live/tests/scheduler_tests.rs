//! E2E scheduler tests. Drive virtual time via `tokio::time::pause()`.
//!
//! `ScheduleConfig` is `#[non_exhaustive]`, so we use `Default::default()`:
//! quiet = 100ms, cap = 500ms.

#![deny(missing_docs)]

use std::path::PathBuf;
use std::time::Duration;

use lspkit_live::{spawn_scheduler, ChangeEvent, ScheduleConfig, Trigger};
use tokio::sync::mpsc;
use tokio::time;

const QUIET: Duration = Duration::from_millis(100);
const CAP: Duration = Duration::from_millis(500);

/// Build a fresh scheduler with default config.
fn build() -> (
    mpsc::UnboundedSender<ChangeEvent>,
    mpsc::UnboundedReceiver<Trigger>,
) {
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
