//! JSON-RPC method dispatcher.
//!
//! Handlers are registered by method name. Every request is associated with a
//! [`CancellationToken`] that fires when the client sends `$/cancelRequest`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::jsonrpc::{JsonRpcError, RequestId};

mod cancellation;
mod interest;
pub use cancellation::CancellationHandle;
use interest::RequestControl;
pub use interest::RequestInterest;

/// Sole registry connecting protocol cancellation to queued and executing work.
type Cancellations = RwLock<HashMap<RequestId, Arc<RequestControl>>>;

/// Object-safe async handler result used by dynamically registered methods.
type HandlerFuture = Pin<Box<dyn std::future::Future<Output = HandlerResult> + Send>>;
/// Shareable type-erased handler invoked with request parameters and lifetime.
type HandlerFn = Arc<dyn Fn(Value, CancellationToken) -> HandlerFuture + Send + Sync>;

/// The two outcomes a request handler can produce.
#[non_exhaustive]
#[derive(Debug)]
pub enum HandlerResult {
    /// Success — value is JSON-serialized into the response's `result`.
    Ok(Value),
    /// Failure — error is serialized into the response's `error`.
    Err(JsonRpcError),
}

/// Errors from dispatching a request.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    /// No handler registered for the method.
    #[error("method not found: {0}")]
    UnknownMethod(String),
    /// Cancellation registration failed, so the request was never scheduled:
    /// a poisoned registry lock is reported instead of skipping registration.
    /// [LSP-PULL-CANCELLATION]
    #[error("cancellation registry lock poisoned; request {0:?} not registered")]
    RegistrationPoisoned(RequestId),
}

/// RAII registration: removes a request's cancellation token on drop, whether
/// the handler completed or the future was dropped before its first poll.
/// [LSP-PULL-CANCELLATION]
struct RegistrationGuard {
    /// Identifies the registry entry owned by this exact request.
    id: RequestId,
    /// Prevents a reused request ID from removing a newer registration.
    control: Arc<RequestControl>,
    /// Shared registry released on completion or dropped work.
    cancellations: Arc<Cancellations>,
}

impl Drop for RegistrationGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.cancellations.write() {
            if guard
                .get(&self.id)
                .is_some_and(|control| Arc::ptr_eq(control, &self.control))
            {
                let _removed = guard.remove(&self.id);
            }
        }
    }
}

/// Routes inbound JSON-RPC method calls to async handlers.
#[derive(Clone, Default)]
pub struct Dispatcher {
    /// Consumer handlers shared with clones and replaced by method name.
    handlers: Arc<RwLock<HashMap<String, HandlerFn>>>,
    /// Cancellation controls registered before any request is scheduled.
    cancellations: Arc<Cancellations>,
}

impl Dispatcher {
    /// New empty dispatcher.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a handler for `method`. Replaces any previously registered handler.
    pub fn register<F, Fut>(&self, method: impl Into<String>, handler: F)
    where
        F: Fn(Value, CancellationToken) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = HandlerResult> + Send + 'static,
    {
        let wrapped: HandlerFn = Arc::new(move |params, cancel| Box::pin(handler(params, cancel)));
        if let Ok(mut guard) = self.handlers.write() {
            let _replaced = guard.insert(method.into(), wrapped);
        }
    }

    /// Prepare a request synchronously and return its execution future.
    ///
    /// The cancellation token is registered in the shared map *before* this
    /// returns, so `$/cancelRequest` cannot race ahead of registration; the
    /// registration is released when the future completes or is dropped.
    /// [LSP-PULL-CANCELLATION]
    ///
    /// # Errors
    /// Returns [`DispatchError::UnknownMethod`] if no handler is registered,
    /// or [`DispatchError::RegistrationPoisoned`] if cancellation registration
    /// failed (the request is not scheduled in that case).
    pub fn prepare(
        &self,
        id: RequestId,
        method: &str,
        params: Value,
    ) -> Result<impl Future<Output = HandlerResult> + Send + 'static, DispatchError> {
        let handler = self
            .handlers
            .read()
            .ok()
            .and_then(|guard| guard.get(method).cloned())
            .ok_or_else(|| DispatchError::UnknownMethod(method.to_owned()))?;
        self.prepare_call(id, move |cancel| handler(params, cancel))
    }

    /// Register application-owned request work in the same cancellation registry.
    /// The returned future releases its registration on completion or drop.
    /// Implements [LSP-PULL-CANCELLATION].
    ///
    /// # Errors
    /// Returns a registration error when the shared registry is unavailable.
    pub fn prepare_call<F, Fut>(
        &self,
        id: RequestId,
        operation: F,
    ) -> Result<impl Future<Output = HandlerResult> + Send + 'static, DispatchError>
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = HandlerResult> + Send + 'static,
    {
        let (control, guard) = self.register_cancellation(id)?;
        Ok(async move {
            let result = operation(control.cancel.clone()).await;
            let result = control.result(result);
            drop(guard);
            result
        })
    }

    /// Register `id` before scheduling and build its RAII release guard.
    /// [LSP-PULL-CANCELLATION]
    ///
    /// # Errors
    /// Returns [`DispatchError::RegistrationPoisoned`] if the registry lock is
    /// poisoned; nothing is registered in that case.
    fn register_cancellation(
        &self,
        id: RequestId,
    ) -> Result<(Arc<RequestControl>, RegistrationGuard), DispatchError> {
        let Ok(mut registry) = self.cancellations.write() else {
            return Err(DispatchError::RegistrationPoisoned(id));
        };
        let control = Arc::clone(registry.entry(id.clone()).or_default());
        let guard = RegistrationGuard {
            id,
            control: Arc::clone(&control),
            cancellations: Arc::clone(&self.cancellations),
        };
        Ok((control, guard))
    }

    /// Dispatch a request. Returns the handler's result.
    ///
    /// The supplied `id` is used to register a cancellation token so a later
    /// `$/cancelRequest` for the same id can trip the handler.
    ///
    /// # Errors
    /// Returns [`DispatchError::UnknownMethod`] if no handler is registered.
    pub async fn dispatch(
        &self,
        id: &RequestId,
        method: &str,
        params: Value,
    ) -> Result<HandlerResult, DispatchError> {
        Ok(self.prepare(id.clone(), method, params)?.await)
    }

    /// Trip every in-flight request's cancellation token, e.g. on shutdown.
    /// Registrations are still released by each request's own guard.
    /// [LSP-PULL-CANCELLATION]
    pub fn cancel_all(&self) {
        if let Ok(guard) = self.cancellations.read() {
            for token in guard.values() {
                token.cancel();
            }
        } else {
            tracing::warn!("Request cancellation registry unavailable during shutdown");
        }
    }

    /// Trip the cancellation token for an in-flight request. No-op if unknown.
    pub fn cancel(&self, id: &RequestId) {
        self.cancellation_handle().cancel(id);
    }

    /// Keep protocol cancellation responsive while foreground handlers are occupied.
    #[must_use]
    pub fn cancellation_handle(&self) -> CancellationHandle {
        CancellationHandle::new(&self.cancellations)
    }
}

impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let handlers = self.handlers.read().map_or(0, |g| g.len());
        let in_flight = self.cancellations.read().map_or(0, |g| g.len());
        f.debug_struct("Dispatcher")
            .field("handlers", &handlers)
            .field("in_flight", &in_flight)
            .finish()
    }
}
