//! JSON-RPC method dispatcher.
//!
//! Handlers are registered by method name. Every request is associated with a
//! [`CancellationToken`] that fires when the client sends `$/cancelRequest`.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::jsonrpc::{JsonRpcError, RequestId};

type HandlerFuture = Pin<Box<dyn std::future::Future<Output = HandlerResult> + Send>>;
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
}

/// Routes inbound JSON-RPC method calls to async handlers.
#[derive(Clone, Default)]
pub struct Dispatcher {
    handlers: Arc<RwLock<HashMap<String, HandlerFn>>>,
    cancellations: Arc<RwLock<HashMap<RequestId, CancellationToken>>>,
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
            guard.insert(method.into(), wrapped);
        }
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
        let handler = self
            .handlers
            .read()
            .ok()
            .and_then(|guard| guard.get(method).cloned())
            .ok_or_else(|| DispatchError::UnknownMethod(method.to_owned()))?;
        let cancel = CancellationToken::new();
        if let Ok(mut guard) = self.cancellations.write() {
            guard.insert(id.clone(), cancel.clone());
        }
        let result = handler(params, cancel).await;
        if let Ok(mut guard) = self.cancellations.write() {
            guard.remove(id);
        }
        Ok(result)
    }

    /// Trip the cancellation token for an in-flight request. No-op if unknown.
    pub fn cancel(&self, id: &RequestId) {
        if let Ok(guard) = self.cancellations.read() {
            if let Some(token) = guard.get(id) {
                token.cancel();
            }
        }
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
