//! Shared test helpers: a minimal `EngineApi` impl for adapter tests.
//!
//! Reused across integration tests. Kept tiny — the adapter never inspects the
//! engine beyond passing it to handlers.

use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use futures_util::stream;
use lspkit::prelude::*;
use tokio_util::sync::CancellationToken;

/// Error type returned by [`FakeEngine`].
#[derive(Debug)]
pub enum FakeError {
    /// The engine was asked to fail on purpose.
    Boom(String),
}

impl fmt::Display for FakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boom(msg) => write!(f, "boom: {msg}"),
        }
    }
}

impl std::error::Error for FakeError {}

/// Trivial in-memory engine for exercising the MCP adapter.
#[derive(Clone)]
pub struct FakeEngine {
    generation: Arc<Mutex<Generation>>,
}

impl FakeEngine {
    /// Build a fake engine wrapped in an `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            generation: Arc::new(Mutex::new(Generation::ZERO)),
        })
    }
}

#[async_trait]
impl EngineApi for FakeEngine {
    type Report = String;
    type Query = String;
    type Error = FakeError;

    fn generation(&self) -> Generation {
        match self.generation.lock() {
            Ok(guard) => *guard,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    async fn report(
        &self,
        query: Self::Query,
        _cancel: CancellationToken,
    ) -> Result<Snapshot<Self::Report>, Self::Error> {
        Ok(Snapshot::new(self.generation(), format!("echo:{query}")))
    }

    async fn rescan(
        &self,
        _scope: RescanScope,
        _progress: Progress,
    ) -> Result<RescanTicket, Self::Error> {
        let mut guard = match self.generation.lock() {
            Ok(value) => value,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = guard.next();
        Ok(RescanTicket::new(*guard))
    }

    fn subscribe(&self) -> GenerationEventStream {
        Box::pin(stream::empty())
    }

    async fn shutdown(&self) -> Result<(), Self::Error> {
        Ok(())
    }
}
