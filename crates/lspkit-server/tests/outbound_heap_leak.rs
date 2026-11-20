//! [LSP-OUTBOUND] [MEM-LIFETIME]: every server-request outcome frees its heap.
//! Single-test binary: the instrumented allocator sees no sibling tests.

use lspkit_server::{JsonRpcError, Message, Outbound, OutboundError, RequestId};
use serde_json::{json, Value};

#[path = "../../../test-support/state.rs"]
mod state;

/// Enough lifetimes per outcome to expose cumulative retention.
const ROUNDS: usize = 32;
/// Heap bytes carried by each request's params and each answer.
const PAYLOAD_BYTES: usize = 16 * 1024;

/// A payload that owns real heap memory.
fn payload() -> Value {
    json!({ "blob": "x".repeat(PAYLOAD_BYTES) })
}

/// A lifetime whose outcome differs from the expected one.
type Mismatch = Box<dyn std::error::Error + Send + Sync>;

/// Fail the lifetime with `what` unless `held`.
fn ensure(held: bool, what: &str) -> Result<(), Mismatch> {
    if held {
        Ok(())
    } else {
        Err(what.into())
    }
}

/// The id the server put on `sent`.
fn sent_id(sent: &Message) -> Result<RequestId, Mismatch> {
    sent.id
        .clone()
        .ok_or_else(|| "a server request carries an id".into())
}

/// Answered, refused, abandoned and unmatched lifetimes, each fully released.
fn lifetimes(outbound: &Outbound) -> Result<(), Mismatch> {
    for _ in 0..ROUNDS {
        let (sent, mut answered) = outbound.request("window/showDocument", payload())?;
        outbound.resolve(Message::response(sent_id(&sent)?, payload()))?;
        ensure(matches!(answered.try_recv(), Ok(Ok(_))), "answered")?;
        let (sent, mut refused) = outbound.request("window/showDocument", payload())?;
        let refusal = JsonRpcError::new(-32603, "refused").with_data(payload());
        outbound.resolve(Message::response_error(sent_id(&sent)?, refusal))?;
        ensure(matches!(refused.try_recv(), Ok(Err(_))), "refused")?;
        let (sent, abandoned) = outbound.request("window/showDocument", payload())?;
        drop(abandoned);
        let late = outbound.resolve(Message::response(sent_id(&sent)?, payload()));
        ensure(
            matches!(late, Err(OutboundError::UnknownId(_))),
            "abandoned",
        )?;
        let foreign = Message::response(RequestId::String("editor".to_owned()), payload());
        ensure(
            matches!(outbound.resolve(foreign), Err(OutboundError::UnknownId(_))),
            "foreign",
        )?;
    }
    Ok(())
}

#[test]
fn every_outbound_outcome_frees_its_heap_before_the_handle_drops() {
    let outbound = Outbound::new();
    lifetimes(&outbound).expect("warm the pending table's capacity");
    state::assert_releases_heap(|| lifetimes(&outbound).expect("measured lifetimes"));
    assert_eq!(outbound.pending_count(), 0);
}
