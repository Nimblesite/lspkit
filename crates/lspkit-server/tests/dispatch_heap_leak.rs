//! [LSP-PULL-CANCELLATION] [MEM-LIFETIME]: terminal errors release actual heap allocations.

use lspkit_server::{
    DispatchError, Dispatcher, HandlerResult, QueuedRequest, RequestId, RequestInterest,
};
use serde_json::Value;

#[path = "../../../test-support/state.rs"]
mod state;

/// Enough complete lifetimes to expose cumulative retention.
const REQUESTS: i64 = 32;
/// Actual heap bytes owned by every registration's input identity.
const INPUT_BYTES: usize = 64 * 1024;
/// One known handler warms the map without retaining request state.
const METHOD: &str = "memory/probe";

/// Populate the registry without retaining a second copy of the input allocations.
fn register_inputs(dispatcher: &Dispatcher) -> Result<Vec<QueuedRequest>, DispatchError> {
    let handle = dispatcher.cancellation_handle();
    (0..REQUESTS)
        .map(|id| {
            handle.queue(
                RequestId::Number(id),
                Some(RequestInterest {
                    method: METHOD.to_owned(),
                    resource: id.to_string(),
                    input: Some("x".repeat(INPUT_BYTES)),
                }),
            )
        })
        .collect()
}

/// Warm map capacity with successful prepared/dropped lifetimes before measuring errors.
fn warmed_dispatcher() -> Result<Dispatcher, DispatchError> {
    let dispatcher = Dispatcher::new();
    dispatcher.register(METHOD, |_, _| async { HandlerResult::Ok(Value::Null) });
    let queued = register_inputs(&dispatcher)?;
    for id in 0..REQUESTS {
        drop(dispatcher.prepare(RequestId::Number(id), METHOD, Value::Null)?);
    }
    drop(queued);
    Ok(dispatcher)
}

#[test]
fn terminal_lookup_errors_free_heap_before_shutdown_or_registry_inspection() {
    let dispatcher = warmed_dispatcher().expect("warm registry capacity");
    state::assert_releases_heap(|| {
        let queued = register_inputs(&dispatcher).expect("register real input allocations");
        for id in 0..REQUESTS {
            let result = dispatcher.prepare(RequestId::Number(id), "unknown", Value::Null);
            assert!(matches!(result, Err(DispatchError::UnknownMethod(_))));
        }
        assert!(
            format!("{dispatcher:?}").contains("in_flight: 0"),
            "the terminal errors released every entry before any guard dropped"
        );
        drop(queued);
    });
}
