//! Request-ID correlation.
//!
//! Out-of-process backends respond asynchronously and out of order. The
//! correlator hands out monotonic request IDs and maps responses back to the
//! waiting future.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};

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
pub enum CorrelationError<R = ()> {
    /// A response arrived for an unknown request ID.
    #[error("unknown request id: {0}")]
    UnknownId(u64),
    /// The receiver was dropped; retains the response so callers can recover it.
    #[error("request {0} already completed")]
    AlreadyResolved(u64, R),
}

/// Pending channel senders, shared only with their correlator owner.
type Pending<R> = Mutex<HashMap<RequestId, oneshot::Sender<R>>>;

/// A response future that releases its registration immediately when abandoned.
/// [MEM-FIX-SCOPE]
#[derive(Debug)]
pub struct PendingResponse<R> {
    /// The registration released by this receiver.
    id: RequestId,
    /// A receiver must not prolong the correlator's lifetime.
    pending: Weak<Pending<R>>,
    /// The actual response channel.
    receiver: oneshot::Receiver<R>,
}

impl<R> PendingResponse<R> {
    /// Attempt to receive a response without waiting.
    ///
    /// # Errors
    /// Returns the channel's empty or closed error when no response is available.
    pub fn try_recv(&mut self) -> Result<R, oneshot::error::TryRecvError> {
        self.receiver.try_recv()
    }
}

impl<R> Future for PendingResponse<R> {
    type Output = Result<R, oneshot::error::RecvError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.receiver).poll(cx)
    }
}

impl<R> Drop for PendingResponse<R> {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.upgrade() {
            let mut pending = pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _removed = pending.remove(&self.id);
        }
    }
}

/// Pairs outbound requests with inbound responses by ID.
#[derive(Default)]
pub struct Correlator<R> {
    /// Allocates identifiers that remain unique for this correlator's lifetime.
    next_id: AtomicU64,
    /// Routes each in-flight response to the future returned by `register`.
    pending: Arc<Pending<R>>,
}

impl<R> Correlator<R> {
    /// New correlator with no pending requests.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            pending: Arc::new(Mutex::new(HashMap::new())),
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
    /// matching response arrives. Dropping the receiver immediately unregisters it.
    pub fn register(&self) -> (RequestId, PendingResponse<R>) {
        let id = RequestId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = oneshot::channel();
        if let Ok(mut guard) = self.pending.lock() {
            let _replaced = guard.insert(id, tx);
        }
        (
            id,
            PendingResponse {
                id,
                pending: Arc::downgrade(&self.pending),
                receiver: rx,
            },
        )
    }

    /// Resolve a pending request with `response`.
    ///
    /// # Errors
    /// Returns [`CorrelationError::UnknownId`] when no pending request
    /// matches (including an abandoned receiver), or [`CorrelationError::AlreadyResolved`]
    /// if the receiver is dropped concurrently after its sender was removed.
    pub fn resolve(&self, id: RequestId, response: R) -> Result<(), CorrelationError<R>> {
        let sender = self
            .pending
            .lock()
            .ok()
            .and_then(|mut guard| guard.remove(&id))
            .ok_or(CorrelationError::UnknownId(id.get()))?;
        sender
            .send(response)
            .map_err(|response| CorrelationError::AlreadyResolved(id.get(), response))
    }
}

impl<R> std::fmt::Debug for Correlator<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Correlator")
            .field("pending", &self.pending_count())
            .finish()
    }
}

// Isolate a closed channel deterministically instead of racing receiver drop and send.
#[cfg(test)]
mod tests {
    use super::{CorrelationError, Correlator};

    #[test]
    fn a_receiver_closed_during_resolution_returns_the_unsent_response() {
        let correlator = Correlator::<u32>::new();
        let (id, mut response) = correlator.register();
        response.receiver.close();
        assert!(matches!(correlator.resolve(id, 99),
            Err(CorrelationError::AlreadyResolved(raw, 99)) if raw == id.get()));
        assert_eq!(correlator.pending_count(), 0);
    }
}
