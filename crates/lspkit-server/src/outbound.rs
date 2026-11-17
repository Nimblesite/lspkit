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
    /// The correlator id no longer fits a JSON-RPC number on the wire.
    #[error("server request id {0} exceeds the largest JSON-RPC number")]
    IdsExhausted(u64),
}

/// Allocates server request ids and routes the editor's answers back.
///
/// Use exactly one handle per connection: ids are unique only within a handle,
/// so two handles on one connection would allocate the same ids.
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
    ///
    /// # Errors
    /// [`OutboundError::IdsExhausted`] once ids pass the largest JSON-RPC
    /// number; the registration is released, never reused.
    pub fn request(
        &self,
        method: impl Into<String>,
        params: Value,
    ) -> Result<(Message, OutboundResponse), OutboundError> {
        let (id, response) = self.correlator.register();
        Ok((
            Message::request(wire_id(id.get())?, method, params),
            response,
        ))
    }

    /// Deliver an inbound response to the request that is waiting for it.
    ///
    /// # Errors
    /// [`OutboundError::NotAResponse`] for requests, notifications and
    /// `"id": null` error responses (they name no request to wake), and
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

/// The wire id for correlator id `raw`; past `i64::MAX` it fails, never collides.
fn wire_id(raw: u64) -> Result<RequestId, OutboundError> {
    i64::try_from(raw)
        .map(RequestId::Number)
        .map_err(|_beyond_wire_numbers| OutboundError::IdsExhausted(raw))
}

/// The correlator number behind a wire id this handle could have allocated.
fn correlated(id: &RequestId) -> Option<u64> {
    match id {
        RequestId::Number(raw) => u64::try_from(*raw).ok(),
        RequestId::String(_) => None,
    }
}

// Reaching 2^63 real registrations is infeasible, so the boundary is isolated here.
#[cfg(test)]
mod tests {
    use super::{correlated, wire_id, OutboundError, RequestId};

    #[test]
    fn ids_past_the_largest_json_rpc_number_fail_instead_of_colliding() {
        let largest = u64::try_from(i64::MAX).expect("i64::MAX is non-negative");
        let last = wire_id(largest).expect("the largest number is still a valid id");
        assert_eq!(last, RequestId::Number(i64::MAX));
        assert_eq!(
            correlated(&last),
            Some(largest),
            "the last valid id round-trips"
        );
        for beyond in [largest + 1, largest + 2, u64::MAX] {
            let error = wire_id(beyond).expect_err("no wire number left");
            assert!(
                matches!(error, OutboundError::IdsExhausted(raw) if raw == beyond),
                "{error:?}"
            );
            assert_eq!(
                error.to_string(),
                format!("server request id {beyond} exceeds the largest JSON-RPC number")
            );
        }
        assert_eq!(wire_id(1).expect("first id"), RequestId::Number(1));
    }
}
