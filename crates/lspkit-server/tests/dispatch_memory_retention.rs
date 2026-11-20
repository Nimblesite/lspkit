//! [LSP-PULL-CANCELLATION] and [MEM-LIFETIME]: terminal request ownership.
//! Each abandoned ingress control owns a 64 KiB input string. The failed
//! assertion observes retained registry entries, not transient RSS or errors.

use lspkit_server::{
    CancellationHandle, DispatchError, Dispatcher, HandlerResult, QueuedRequest, RequestId,
    RequestInterest,
};
use serde_json::Value;

/// Repeat terminal operations to expose accumulation, rather than a single cached entry.
const REQUESTS: i64 = 32;
/// Owned input bytes retained by every pre-registered cancellation control.
const INPUT_BYTES: usize = 64 * 1024;
/// Unknown method forces the early error before a registration guard exists.
const UNKNOWN_METHOD: &str = "textDocument/neverRegistered";
/// Real handler used by both successful lifetime controls.
const KNOWN_METHOD: &str = "textDocument/known";

/// A real registered handler without any captured dispatcher or test allocations.
fn known_dispatcher() -> Dispatcher {
    let dispatcher = Dispatcher::new();
    dispatcher.register(KNOWN_METHOD, |params, _cancel| async move {
        HandlerResult::Ok(params)
    });
    dispatcher
}

/// Compare the public live-registry snapshot directly; no source or Debug parsing.
fn assert_released(actual: &Dispatcher, empty: &Dispatcher) {
    assert_eq!(format!("{actual:?}"), format!("{empty:?}"),
        "terminal requests retained registry entries, each owning a {INPUT_BYTES}-byte input allocation");
}

#[test]
fn unknown_method_releases_pre_registered_interest() {
    let dispatcher = Dispatcher::new();
    let queued = queue(&dispatcher.cancellation_handle()).expect("queue the requests");
    for id in 0..REQUESTS {
        let result = dispatcher.prepare(RequestId::Number(id), UNKNOWN_METHOD, Value::Null);
        assert!(matches!(result, Err(DispatchError::UnknownMethod(_))));
    }
    dispatcher.cancel_all();
    assert_released(&dispatcher, &Dispatcher::new());
    drop(queued);
}

#[tokio::test]
async fn completed_requests_release_pre_registered_interest() {
    let dispatcher = known_dispatcher();
    let queued = queue(&dispatcher.cancellation_handle()).expect("queue the requests");
    for id in 0..REQUESTS {
        let result = dispatcher
            .prepare(RequestId::Number(id), KNOWN_METHOD, Value::Null)
            .expect("known method prepares")
            .await;
        assert!(matches!(result, HandlerResult::Ok(Value::Null)));
    }
    assert_released(&dispatcher, &known_dispatcher());
    drop(queued);
}

#[test]
fn dropped_futures_release_pre_registered_interest() {
    let dispatcher = known_dispatcher();
    let queued = queue(&dispatcher.cancellation_handle()).expect("queue the requests");
    for id in 0..REQUESTS {
        let future = dispatcher
            .prepare(RequestId::Number(id), KNOWN_METHOD, Value::Null)
            .expect("known method prepares");
        drop(future);
    }
    assert_released(&dispatcher, &known_dispatcher());
    drop(queued);
}

/// Queue owned heap strings through the public protocol handle.
fn queue(handle: &CancellationHandle) -> Result<Vec<QueuedRequest>, DispatchError> {
    (0..REQUESTS)
        .map(|id| {
            let interest = RequestInterest {
                method: KNOWN_METHOD.to_owned(),
                resource: format!("file:///memory-audit/{id}"),
                input: Some("x".repeat(INPUT_BYTES)),
            };
            handle.queue(RequestId::Number(id), Some(interest))
        })
        .collect()
}

/// [MEM-LIFETIME-QUEUED-REQUEST] A consumer that rejects a queued request itself.
#[test]
fn queued_requests_abandoned_before_prepare_release_their_interest() {
    let dispatcher = known_dispatcher();
    let queued = queue(&dispatcher.cancellation_handle()).expect("queue the requests");
    assert_eq!(i64::try_from(queued.len()).ok(), Some(REQUESTS));
    assert!(format!("{dispatcher:?}").contains(&format!("in_flight: {REQUESTS}")));
    drop(queued);
    assert_released(&dispatcher, &known_dispatcher());
}

/// [MEM-LIFETIME-QUEUED-REQUEST] A held guard keeps a queued cancellation for the
/// prepared request, which then releases it on completion.
#[tokio::test]
async fn a_held_queue_guard_keeps_queued_cancellation_until_prepared() {
    let dispatcher = Dispatcher::new();
    dispatcher.register(KNOWN_METHOD, |_params, cancel| async move {
        HandlerResult::Ok(Value::Bool(cancel.is_cancelled()))
    });
    let handle = dispatcher.cancellation_handle();
    for guard in queue(&handle).expect("queue the requests") {
        handle.cancel(guard.id());
        let future = dispatcher
            .prepare(guard.id().clone(), KNOWN_METHOD, Value::Null)
            .expect("known method prepares");
        drop(guard);
        let result = future.await;
        assert!(
            matches!(&result, HandlerResult::Ok(Value::Bool(true))),
            "the queued cancellation reached the prepared request: {result:?}"
        );
    }
    assert_eq!(
        format!("{dispatcher:?}"),
        "Dispatcher { handlers: 1, in_flight: 0 }",
        "completed requests released their queued controls"
    );
}
