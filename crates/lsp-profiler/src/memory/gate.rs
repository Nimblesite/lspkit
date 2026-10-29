//! The steps in flight on one memory session, and stopping them. Implements
//! [PROF-MEMORY-STOP].
//!
//! A step can wait on its target indefinitely: a direct runtime reads the live
//! heap, which a frozen, paused or dying program never yields. Stopping the
//! session must not wait on that. It ends every step in flight the way
//! cancellation does, by dropping the step's future, which terminates each tool
//! the step started ([PROF-HOST-CANCEL]), then waits until every one has
//! answered, so nothing touches the session after stop removes its state.

use std::future::Future;
use std::sync::Arc;

use tokio::sync::{watch, OwnedRwLockReadGuard, RwLock};

use super::MemoryError;

/// Admits steps to one session until it stops.
#[derive(Debug, Clone)]
pub(crate) struct StepGate {
    /// Held shared by every step in flight, and exclusively by a stop.
    steps: Arc<RwLock<()>>,
    /// True once the session is stopping.
    stopping: Arc<watch::Sender<bool>>,
}

/// One admitted step: a stop waits for it, and ends it once the stop begins.
#[derive(Debug)]
pub(crate) struct StepPass {
    /// Keeps a stop waiting until this step has answered.
    _held: OwnedRwLockReadGuard<()>,
    /// Observes the session starting to stop.
    stopping: watch::Receiver<bool>,
    /// The session the step belongs to, for the error that ends it.
    session_id: String,
}

impl StepGate {
    /// A gate admitting steps.
    pub(crate) fn new() -> Self {
        Self {
            steps: Arc::new(RwLock::new(())),
            stopping: Arc::new(watch::channel(false).0),
        }
    }

    /// Admit a step on `session_id`, or refuse it once the session is stopping.
    pub(crate) async fn admit(&self, session_id: &str) -> Result<StepPass, MemoryError> {
        let held = Arc::clone(&self.steps).read_owned().await;
        let stopping = self.stopping.subscribe();
        if *stopping.borrow() {
            return Err(MemoryError::Stopped(session_id.to_owned()));
        }
        Ok(StepPass {
            _held: held,
            stopping,
            session_id: session_id.to_owned(),
        })
    }

    /// End every step in flight and wait until each one has answered.
    pub(crate) async fn stop(&self) {
        let _ = self.stopping.send_replace(true);
        drop(self.steps.write().await);
    }
}

impl StepPass {
    /// Run `step` until it answers or its session starts stopping, whichever is first.
    pub(crate) async fn run<T, E: From<MemoryError>>(
        mut self,
        step: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E> {
        let session_id = self.session_id.clone();
        tokio::select! {
            biased;
            _ = self.stopping.wait_for(|stopping| *stopping) => Err(MemoryError::Stopped(session_id).into()),
            answer = step => answer,
        }
    }
}
