//! Integration tests for the `EngineApi` trait via a fake implementation.

use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use futures_util::stream;
use futures_util::StreamExt;
use lspkit::prelude::*;
use tokio_util::sync::CancellationToken;

/// Errors a `FakeEngine` may return.
#[derive(Debug)]
enum FakeError {
    /// The engine has been shut down.
    Shutdown,
    /// The operation was cancelled.
    Cancelled,
}

impl fmt::Display for FakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shutdown => write!(f, "engine has been shut down"),
            Self::Cancelled => write!(f, "operation cancelled"),
        }
    }
}

impl std::error::Error for FakeError {}

/// A fake engine for exercising the `EngineApi` contract.
struct FakeEngine {
    state: Mutex<State>,
    events: Mutex<Vec<GenerationEvent>>,
}

struct State {
    generation: Generation,
    shut_down: bool,
}

impl FakeEngine {
    fn new() -> Arc<Self> {
        let initial = GenerationEvent::new(Generation::ZERO, Cause::Initial);
        Arc::new(Self {
            state: Mutex::new(State {
                generation: Generation::ZERO,
                shut_down: false,
            }),
            events: Mutex::new(vec![initial]),
        })
    }
}

#[async_trait]
impl EngineApi for FakeEngine {
    type Report = Vec<String>;
    type Query = String;
    type Error = FakeError;

    fn generation(&self) -> Generation {
        match self.state.lock() {
            Ok(guard) => guard.generation,
            Err(poisoned) => poisoned.into_inner().generation,
        }
    }

    async fn report(
        &self,
        query: Self::Query,
        cancel: CancellationToken,
    ) -> Result<Snapshot<Self::Report>, Self::Error> {
        let shut_down = match self.state.lock() {
            Ok(guard) => guard.shut_down,
            Err(poisoned) => poisoned.into_inner().shut_down,
        };
        if shut_down {
            return Err(FakeError::Shutdown);
        }
        if cancel.is_cancelled() {
            return Err(FakeError::Cancelled);
        }
        let generation = self.generation();
        Ok(Snapshot::new(generation, vec![query]))
    }

    async fn rescan(
        &self,
        scope: RescanScope,
        progress: Progress,
    ) -> Result<RescanTicket, Self::Error> {
        progress.report("rescan start", Some(0));
        let mut guard = match self.state.lock() {
            Ok(value) => value,
            Err(poisoned) => poisoned.into_inner(),
        };
        if guard.shut_down {
            return Err(FakeError::Shutdown);
        }
        let cause = if let RescanScope::Paths(_) = scope {
            Cause::External
        } else {
            Cause::Rescan
        };
        guard.generation = guard.generation.next();
        let new_generation = guard.generation;
        drop(guard);
        progress.report("rescan complete", Some(100));
        if let Ok(mut events) = self.events.lock() {
            events.push(GenerationEvent::new(new_generation, cause));
        }
        Ok(RescanTicket::new(new_generation))
    }

    fn subscribe(&self) -> GenerationEventStream {
        let events = match self.events.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        Box::pin(stream::iter(events))
    }

    async fn shutdown(&self) -> Result<(), Self::Error> {
        let mut guard = match self.state.lock() {
            Ok(value) => value,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.shut_down = true;
        Ok(())
    }
}

#[tokio::test]
async fn engine_starts_at_zero_generation() {
    let engine = FakeEngine::new();
    assert_eq!(engine.generation(), Generation::ZERO);
}

#[tokio::test]
async fn report_returns_snapshot_at_current_generation() {
    let engine = FakeEngine::new();
    let snapshot = engine
        .report("hello".to_string(), CancellationToken::new())
        .await
        .expect("report should succeed");
    assert_eq!(snapshot.generation, Generation::ZERO);
    assert_eq!(snapshot.data, vec!["hello".to_string()]);
}

#[tokio::test]
async fn report_observes_cancellation_token() {
    let engine = FakeEngine::new();
    let token = CancellationToken::new();
    token.cancel();
    let outcome = engine.report("q".to_string(), token).await;
    assert!(
        matches!(outcome, Err(FakeError::Cancelled)),
        "expected cancelled error, got {outcome:?}"
    );
}

#[tokio::test]
async fn rescan_all_advances_generation_and_emits_event() {
    let engine = FakeEngine::new();
    let ticket = engine
        .rescan(RescanScope::All, Progress::noop())
        .await
        .expect("rescan should succeed");
    assert_eq!(ticket.generation(), Generation::new(1));
    assert_eq!(engine.generation(), Generation::new(1));
}

#[tokio::test]
async fn rescan_paths_uses_external_cause() {
    let engine = FakeEngine::new();
    let paths = vec!["/tmp/a".into(), "/tmp/b".into()];
    let ticket = engine
        .rescan(RescanScope::Paths(paths), Progress::default())
        .await
        .expect("rescan should succeed");
    assert_eq!(ticket.generation(), Generation::new(1));
    let mut subscription = engine.subscribe();
    let mut causes = Vec::new();
    while let Some(event) = subscription.next().await {
        causes.push(format!("{:?}", event.cause));
    }
    assert!(causes.iter().any(|c| c.contains("Initial")));
    assert!(causes.iter().any(|c| c.contains("External")));
}

#[tokio::test]
async fn subscribe_yields_initial_event_before_rescan() {
    let engine = FakeEngine::new();
    let mut subscription = engine.subscribe();
    let first = subscription.next().await.expect("at least one event");
    assert_eq!(first.generation, Generation::ZERO);
    assert!(matches!(first.cause, Cause::Initial));
}

#[tokio::test]
async fn subscribe_collects_multiple_events_after_rescans() {
    let engine = FakeEngine::new();
    let _ = engine
        .rescan(RescanScope::All, Progress::noop())
        .await
        .expect("first rescan");
    let _ = engine
        .rescan(RescanScope::All, Progress::noop())
        .await
        .expect("second rescan");
    let mut subscription = engine.subscribe();
    let mut collected = Vec::new();
    while let Some(event) = subscription.next().await {
        collected.push(event.generation);
    }
    assert_eq!(collected.len(), 3);
    assert_eq!(collected[0], Generation::ZERO);
    assert_eq!(collected[1], Generation::new(1));
    assert_eq!(collected[2], Generation::new(2));
}

#[tokio::test]
async fn shutdown_blocks_further_calls() {
    let engine = FakeEngine::new();
    engine.shutdown().await.expect("shutdown");
    let report_outcome = engine.report("q".into(), CancellationToken::new()).await;
    assert!(
        matches!(report_outcome, Err(FakeError::Shutdown)),
        "expected shutdown error from report, got {report_outcome:?}"
    );
    let rescan_outcome = engine.rescan(RescanScope::All, Progress::noop()).await;
    assert!(
        matches!(rescan_outcome, Err(FakeError::Shutdown)),
        "expected shutdown error from rescan, got {rescan_outcome:?}"
    );
}

#[tokio::test]
async fn fake_error_display_messages() {
    let shutdown = FakeError::Shutdown.to_string();
    let cancelled = FakeError::Cancelled.to_string();
    assert!(shutdown.contains("shut down"));
    assert!(cancelled.contains("cancelled"));
}
