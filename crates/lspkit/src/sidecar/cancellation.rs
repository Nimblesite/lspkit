//! Cancel one active IPC request and retire its terminal reply. [LSP-PULL-CANCELLATION]
use std::future::Future;
use tokio_util::sync::CancellationToken;

/// Cancellation of a caller, distinct from a transport or compiler failure.
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("sidecar request cancelled")]
pub struct RequestCancelled;

/// The terminal reply is consumed even when its original caller was cancelled.
#[derive(Debug)]
pub enum Reply<T> {
    /// The request completed before cancellation.
    Complete(T),
    /// Cancellation was sent and this terminal reply retires the request.
    Cancelled(T),
}

/// Codec-specific control after a complete request frame has been written.
pub trait Exchange: Send {
    /// The consumer's actual decoded wire envelope.
    type Response: Send;
    /// Receive one terminal response; dropping this future must preserve framing.
    fn receive(&mut self) -> impl Future<Output = Result<Self::Response, String>> + Send;
    /// Send a cancellation notification identifying the current request.
    fn cancel(&mut self) -> impl Future<Output = Result<(), String>> + Send;
}

/// Race only cancellation-safe work, such as acquiring a lock or receiving a frame.
///
/// # Errors
/// Returns cancellation without polling new work after an already-cancelled token.
pub async fn until_cancelled<F: Future>(
    future: F,
    cancellation: Option<&CancellationToken>,
) -> Result<F::Output, RequestCancelled> {
    let Some(cancellation) = cancellation else {
        return Ok(future.await);
    };
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(RequestCancelled),
        result = future => Ok(result),
    }
}

/// Preserve response correlation before the next serialized request begins.
///
/// # Errors
/// Returns the consumer's read or cancellation-write failure without fabricating a reply.
pub async fn receive<T: Exchange>(
    exchange: &mut T,
    cancellation: Option<&CancellationToken>,
) -> Result<Reply<T::Response>, String> {
    if let Ok(response) = until_cancelled(exchange.receive(), cancellation).await {
        response.map(Reply::Complete)
    } else {
        exchange.cancel().await?;
        exchange.receive().await.map(Reply::Cancelled)
    }
}
