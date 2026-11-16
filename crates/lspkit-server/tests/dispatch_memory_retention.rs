//! [LSP-PULL-CANCELLATION] and [MEM-AUDIT-SCOPE]: terminal request ownership.
//! Each abandoned ingress control owns a 64 KiB input string. The failed
//! assertion observes retained registry entries, not transient RSS or errors.

use lspkit_server::{
    CancellationHandle, DispatchError, Dispatcher, HandlerResult, RequestId, RequestInterest,
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

/// Register owned heap strings through the public protocol handle.
fn pre_register(handle: &CancellationHandle) {
    for id in 0..REQUESTS {
        let interest = RequestInterest {
            method: UNKNOWN_METHOD.to_owned(),
            resource: format!("file:///memory-audit/{id}"),
            input: Some("x".repeat(INPUT_BYTES)),
        };
        assert!(handle
            .register(RequestId::Number(id), Some(interest))
            .is_ok());
    }
}

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
    pre_register(&dispatcher.cancellation_handle());
    for id in 0..REQUESTS {
        let result = dispatcher.prepare(RequestId::Number(id), UNKNOWN_METHOD, Value::Null);
        assert!(matches!(result, Err(DispatchError::UnknownMethod(_))));
    }
    dispatcher.cancel_all();
    assert_released(&dispatcher, &Dispatcher::new());
}

#[tokio::test]
async fn completed_requests_release_pre_registered_interest() {
    let dispatcher = known_dispatcher();
    pre_register(&dispatcher.cancellation_handle());
    for id in 0..REQUESTS {
        let result = dispatcher
            .prepare(RequestId::Number(id), KNOWN_METHOD, Value::Null)
            .expect("known method prepares")
            .await;
        assert!(matches!(result, HandlerResult::Ok(Value::Null)));
    }
    assert_released(&dispatcher, &known_dispatcher());
}

#[test]
fn dropped_futures_release_pre_registered_interest() {
    let dispatcher = known_dispatcher();
    pre_register(&dispatcher.cancellation_handle());
    for id in 0..REQUESTS {
        let future = dispatcher
            .prepare(RequestId::Number(id), KNOWN_METHOD, Value::Null)
            .expect("known method prepares");
        drop(future);
    }
    assert_released(&dispatcher, &known_dispatcher());
}
