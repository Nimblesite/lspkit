//! Session lifecycle.
//!
//! Wires a [`crate::watcher::FileWatcher`] to a [`crate::scheduler`] and an
//! analyzer callback. The session owns the generation counter that
//! [`lspkit::EngineApi`] implementations advance on every successful pass.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use lspkit::{Cause, Generation, GenerationEvent};
use tokio::sync::broadcast;

use crate::scheduler::Trigger;

/// Result of a single analysis pass.
pub type AnalyzerResult = Result<(), AnalyzerError>;

/// Errors emitted by an analyzer callback.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum AnalyzerError {
    /// The analyzer encountered an unrecoverable problem; the session
    /// keeps running but the generation is not advanced.
    #[error("analyzer failed: {0}")]
    Failed(String),
}

/// A pluggable analyzer driven by the session.
#[async_trait]
pub trait Analyzer: Send + Sync + 'static {
    /// Called once per coalesced [`Trigger`]. Implementations should rebuild
    /// or incrementally update their internal state.
    async fn analyze(&self, trigger: Trigger) -> AnalyzerResult;
}

/// Session that drives an [`Analyzer`] from a scheduler stream.
#[derive(Clone)]
pub struct Session {
    generation: Arc<AtomicU64>,
    events: broadcast::Sender<GenerationEvent>,
}

impl Session {
    /// New session at [`Generation::ZERO`].
    #[must_use]
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(32);
        Self { generation: Arc::new(AtomicU64::new(0)), events }
    }

    /// Current generation.
    #[must_use]
    pub fn generation(&self) -> Generation {
        Generation::new(self.generation.load(Ordering::SeqCst))
    }

    /// Subscribe to generation events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<GenerationEvent> {
        self.events.subscribe()
    }

    /// Bump the generation and broadcast a `Rescan` event.
    #[must_use]
    pub fn advance(&self, cause: Cause) -> Generation {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let event = GenerationEvent::new(Generation::new(next), cause);
        let _ = self.events.send(event);
        Generation::new(next)
    }

    /// Drive the session: read triggers, run the analyzer, advance generation
    /// on success. Returns when `triggers` closes.
    pub async fn run<A: Analyzer>(
        self,
        mut triggers: tokio::sync::mpsc::UnboundedReceiver<Trigger>,
        analyzer: A,
    ) {
        while let Some(trigger) = triggers.recv().await {
            match analyzer.analyze(trigger).await {
                Ok(()) => {
                    let _ = self.advance(Cause::Rescan);
                }
                Err(err) => {
                    tracing::warn!(error = %err, "analyzer pass failed");
                }
            }
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("generation", &self.generation()).finish()
    }
}
