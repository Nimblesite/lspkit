//! E2E session tests using mock analyzers.
//!
//! `Trigger` is `#[non_exhaustive]`, so we obtain real triggers from the
//! scheduler under paused virtual time.

#![deny(missing_docs)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lspkit::{Cause, Generation};
use lspkit_server::live::{
    spawn_scheduler, Analyzer, AnalyzerError, AnalyzerResult, ChangeEvent, ScheduleConfig, Session,
    Trigger,
};
use tokio::sync::mpsc;
use tokio::time;

/// Analyzer that always succeeds; counts invocations.
struct OkAnalyzer {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Analyzer for OkAnalyzer {
    async fn analyze(&self, _trigger: Trigger) -> AnalyzerResult {
        let _previous_calls = self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// Analyzer that always fails.
struct FailAnalyzer;

#[async_trait]
impl Analyzer for FailAnalyzer {
    async fn analyze(&self, _trigger: Trigger) -> AnalyzerResult {
        Err(AnalyzerError::Failed("boom".into()))
    }
}

/// Yield repeatedly so the spawned scheduler observes all pending work.
async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

/// Produce `count` triggers using the scheduler under virtual time.
async fn produce_triggers(count: usize) -> Option<Vec<Trigger>> {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut triggers_rx = spawn_scheduler(rx, ScheduleConfig::default());
    let mut out = Vec::with_capacity(count);
    for idx in 0..count {
        tx.send(ChangeEvent::Touched(vec![PathBuf::from(format!("f{idx}"))]))
            .ok()?;
        settle().await;
        time::advance(Duration::from_millis(150)).await;
        settle().await;
        let trig = triggers_rx.recv().await?;
        out.push(trig);
    }
    Some(out)
}

#[tokio::test]
async fn new_session_starts_at_generation_zero() {
    let session = Session::new();
    assert_eq!(session.generation(), Generation::ZERO);
    let dbg = format!("{session:?}");
    assert!(dbg.contains("Session"));
    assert!(dbg.contains("generation"));
}

#[tokio::test]
async fn default_matches_new() {
    let a = Session::new();
    let b = Session::default();
    assert_eq!(a.generation(), b.generation());
}

#[tokio::test]
async fn advance_bumps_generation_and_emits_event() {
    let session = Session::new();
    let mut events = session.subscribe();

    let next = session.advance(Cause::Initial);
    assert_eq!(next, Generation::new(1));
    assert_eq!(session.generation(), Generation::new(1));

    let received = events.recv().await.expect("event");
    assert_eq!(received.generation, Generation::new(1));
    assert!(matches!(received.cause, Cause::Initial));

    let next2 = session.advance(Cause::External);
    assert_eq!(next2, Generation::new(2));
    let received2 = events.recv().await.expect("event2");
    assert!(matches!(received2.cause, Cause::External));
}

#[tokio::test]
async fn clone_shares_state() {
    let a = Session::new();
    let b = a.clone();
    let _ = a.advance(Cause::Rescan);
    assert_eq!(a.generation(), b.generation());
    assert_eq!(b.generation(), Generation::new(1));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn run_advances_generation_on_each_success() {
    let triggers = produce_triggers(3).await.expect("three real triggers");

    let session = Session::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let analyzer = OkAnalyzer {
        calls: Arc::clone(&calls),
    };
    let (tx, rx) = mpsc::unbounded_channel();
    for t in triggers {
        tx.send(t).expect("send");
    }
    drop(tx);

    let observer = session.clone();
    session.run(rx, analyzer).await;

    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(observer.generation(), Generation::new(3));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn run_does_not_advance_on_analyzer_failure() {
    let triggers = produce_triggers(2).await.expect("two real triggers");

    let session = Session::new();
    let (tx, rx) = mpsc::unbounded_channel();
    for t in triggers {
        tx.send(t).expect("send");
    }
    drop(tx);

    let observer = session.clone();
    session.run(rx, FailAnalyzer).await;

    assert_eq!(observer.generation(), Generation::ZERO);
}

#[tokio::test]
async fn run_handles_empty_trigger_stream() {
    let session = Session::new();
    let (tx, rx) = mpsc::unbounded_channel::<Trigger>();
    drop(tx);
    let observer = session.clone();
    session
        .run(
            rx,
            OkAnalyzer {
                calls: Arc::new(AtomicUsize::new(0)),
            },
        )
        .await;
    assert_eq!(observer.generation(), Generation::ZERO);
}

#[tokio::test]
async fn analyzer_error_display_contains_message() {
    let err = AnalyzerError::Failed("kaput".into());
    let s = format!("{err}");
    assert!(s.contains("kaput"));
    assert!(format!("{err:?}").contains("Failed"));
}

#[tokio::test]
async fn subscribe_delivers_to_late_subscribers_after_advance() {
    let session = Session::new();
    let _ = session.advance(Cause::Rescan);
    let mut late = session.subscribe();
    let _ = session.advance(Cause::Rescan);
    let evt = late.recv().await.expect("evt");
    assert_eq!(evt.generation, Generation::new(2));
}
