//! Replacement interest shares the dispatcher registry. [LSP-PULL-CANCELLATION]
use std::sync::atomic::{AtomicU8, Ordering};

use serde_json::json;

use super::{CancellationToken, HandlerResult, JsonRpcError};

/// Protocol metadata identifying replaceable work, never a result cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestInterest {
    /// Request kind whose work can replace an earlier request.
    pub method: String,
    /// Consumer-canonicalized resource identity.
    pub resource: String,
    /// Opaque identity of the input the client previously observed.
    pub input: Option<String>,
}

impl RequestInterest {
    /// Match a consumer's method and canonical resource independently of cached result IDs.
    pub(super) fn matches(&self, method: &str, resource: &str) -> bool {
        self.method == method && self.resource == resource
    }

    /// Changed interest replaces only work of the same kind for the same resource.
    pub(super) fn supersedes(&self, previous: &Self) -> bool {
        self.matches(&previous.method, &previous.resource) && self.input != previous.input
    }
}

/// A queued or executing request has exactly one cancellation control.
#[derive(Debug, Default)]
pub(super) struct RequestControl {
    /// Signals executing consumer work after the first interruption.
    pub(super) cancel: CancellationToken,
    /// Identifies replaceable method/resource/input scope without caching results.
    pub(super) interest: Option<RequestInterest>,
    /// Atomically records whether cancellation or supersession arrived first.
    pub(super) reason: AtomicU8,
}

impl RequestControl {
    /// The first interruption wins, including a client cancellation preceding supersession.
    fn interrupt(&self, reason: u8) {
        if self
            .reason
            .compare_exchange(0, reason, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.cancel.cancel();
        }
    }

    /// Marks client cancellation unless supersession already won.
    pub(super) fn cancel(&self) {
        self.interrupt(1);
    }

    /// Marks replacement unless client cancellation already won.
    pub(super) fn supersede(&self) {
        self.interrupt(2);
    }

    /// Converts superseded completion into the retriggerable protocol error.
    pub(super) fn result(&self, result: HandlerResult) -> HandlerResult {
        if self.reason.load(Ordering::SeqCst) == 2 {
            HandlerResult::Err(
                JsonRpcError::new(-32802, "Request superseded by newer client interest")
                    .with_data(json!({"retriggerRequest": true})),
            )
        } else {
            result
        }
    }
}
