//! Request-ID correlation.
//!
//! Out-of-process backends respond asynchronously and out of order. The
//! correlator hands out monotonic request IDs and maps responses back to the
//! waiting future.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use tokio::sync::oneshot;

/// Monotonic request identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(u64);

impl RequestId {
    /// Raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Errors from the correlator.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum CorrelationError {
    /// A response arrived for an unknown request ID.
    #[error("unknown request id: {0}")]
    UnknownId(u64),
    /// The pending future was already resolved.
    #[error("request {0} already completed")]
    AlreadyResolved(u64),
}

/// Pairs outbound requests with inbound responses by ID.
#[derive(Default)]
pub struct Correlator<R> {
    next_id: AtomicU64,
    pending: Mutex<HashMap<RequestId, oneshot::Sender<R>>>,
}

impl<R> Correlator<R> {
    /// New correlator with no pending requests.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Number of pending requests.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.lock().map_or(0, |g| g.len())
    }
}

impl<R: Send + 'static> Correlator<R> {
    /// Allocate a new request ID and a receiver that will resolve when the
    /// matching response arrives.
    pub fn register(&self) -> (RequestId, oneshot::Receiver<R>) {
        let id = RequestId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = oneshot::channel();
        if let Ok(mut guard) = self.pending.lock() {
            guard.insert(id, tx);
        }
        (id, rx)
    }

    /// Resolve a pending request with `response`.
    ///
    /// # Errors
    /// Returns [`CorrelationError::UnknownId`] when no pending request
    /// matches, or [`CorrelationError::AlreadyResolved`] if the receiver was
    /// dropped.
    pub fn resolve(&self, id: RequestId, response: R) -> Result<(), CorrelationError> {
        let sender = self
            .pending
            .lock()
            .ok()
            .and_then(|mut guard| guard.remove(&id))
            .ok_or(CorrelationError::UnknownId(id.get()))?;
        sender
            .send(response)
            .map_err(|_| CorrelationError::AlreadyResolved(id.get()))
    }
}

impl<R> std::fmt::Debug for Correlator<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Correlator")
            .field("pending", &self.pending_count())
            .finish()
    }
}
