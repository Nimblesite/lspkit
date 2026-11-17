//! Server→client requests (`window/showDocument`, `workspace/diagnostic/refresh`).
//! [LSP-OUTBOUND]
//!
//! Ids come from the leak-proofed sidecar correlator, so an abandoned wait
//! unregisters itself on drop exactly as [SIDECAR-REQUEST-LIFETIME] requires.

use lspkit::sidecar::{CorrelationError, Correlator, PendingResponse};
use serde_json::Value;

use crate::jsonrpc::{JsonRpcError, Message, RequestId};

/// What the editor answered: its `result`, or its JSON-RPC error.
pub type OutboundResult = Result<Value, JsonRpcError>;

/// Resolves when the editor answers; dropping it unregisters the request.
pub type OutboundResponse = PendingResponse<OutboundResult>;

/// Errors from matching an inbound message to a server request.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum OutboundError {
    /// The message is a request or notification, not a response.
    #[error("message is not a response")]
    NotAResponse,
    /// No waiting server request has this id: foreign, answered or abandoned.
    #[error("no pending server request for id {0:?}")]
    UnknownId(RequestId),
}

/// Allocates server request ids and routes the editor's answers back.
#[derive(Debug)]
pub struct Outbound {
    /// The only pending table; entries leave when answered or abandoned.
    correlator: Correlator<OutboundResult>,
}

impl Outbound {
    /// A handle with no pending requests.
    #[must_use]
    pub fn new() -> Self {
        Self {
            correlator: Correlator::new(),
        }
    }

    /// Build the request frame to write and the wait for its answer.
    #[must_use]
    pub fn request(&self, method: impl Into<String>, params: Value) -> (Message, OutboundResponse) {
        let (id, response) = self.correlator.register();
        // Ids count up from 1; i64::MAX would take 2^63 requests to reach.
        let wire = RequestId::Number(i64::try_from(id.get()).unwrap_or(i64::MAX));
        (Message::request(wire, method, params), response)
    }

    /// Deliver an inbound response to the request that is waiting for it.
    ///
    /// # Errors
    /// [`OutboundError::NotAResponse`] for requests and notifications, and
    /// [`OutboundError::UnknownId`] when no waiting request has the id.
    pub fn resolve(&self, message: Message) -> Result<(), OutboundError> {
        let (Some(id), None) = (message.id, message.method) else {
            return Err(OutboundError::NotAResponse);
        };
        let answer = message
            .error
            .map_or_else(|| Ok(message.result.unwrap_or(Value::Null)), Err);
        let Some(raw) = correlated(&id) else {
            return Err(OutboundError::UnknownId(id));
        };
        self.correlator.resolve(raw.into(), answer).map_err(
            |_answered_or_abandoned: CorrelationError<OutboundResult>| OutboundError::UnknownId(id),
        )
    }

    /// Requests still waiting for an answer.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.correlator.pending_count()
    }
}

impl Default for Outbound {
    fn default() -> Self {
        Self::new()
    }
}

/// The correlator number behind a wire id this handle could have allocated.
fn correlated(id: &RequestId) -> Option<u64> {
    match id {
        RequestId::Number(raw) => u64::try_from(*raw).ok(),
        RequestId::String(_) => None,
    }
}
