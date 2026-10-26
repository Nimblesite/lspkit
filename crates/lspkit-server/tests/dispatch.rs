//! End-to-end tests for [`lspkit_server::Dispatcher`].

use std::sync::Arc;
use std::time::Duration;

use lspkit_server::{DispatchError, Dispatcher, HandlerResult, JsonRpcError, RequestId};
use serde_json::{json, Value};
use tokio::sync::Notify;

/// The `Ok` payload of a handler result; tests `expect` on it by step name.
fn ok_value(result: HandlerResult) -> Result<Value, String> {
    match result {
        HandlerResult::Ok(value) => Ok(value),
        _ => Err(format!("expected Ok variant: {result:?}")),
    }
}

/// The error code of a handler result; tests `expect` on it by step name.
fn err_code(result: HandlerResult) -> Result<i32, String> {
    match result {
        HandlerResult::Err(err) => Ok(err.code),
        _ => Err(format!("expected Err variant: {result:?}")),
    }
}

#[tokio::test]
async fn unknown_method_returns_error() {
    let dispatcher = Dispatcher::new();
    let id = RequestId::Number(1);
    let err = dispatcher
        .dispatch(&id, "no/such/method", Value::Null)
        .await
        .expect_err("must reject");
    if let DispatchError::UnknownMethod(method) = err {
        assert_eq!(method, "no/such/method");
    }
}

#[tokio::test]
async fn registered_handler_receives_params_and_returns_ok() {
    let dispatcher = Dispatcher::new();
    dispatcher.register("echo", |params: Value, _cancel| async move {
        HandlerResult::Ok(params)
    });
    let id = RequestId::Number(7);
    let result = dispatcher
        .dispatch(&id, "echo", json!({"hello": "world"}))
        .await
        .expect("dispatch ok");
    assert_eq!(
        ok_value(result).expect("handler answered Ok"),
        json!({"hello": "world"})
    );
}

#[tokio::test]
async fn registered_handler_can_return_jsonrpc_error() {
    let dispatcher = Dispatcher::new();
    dispatcher.register("bad", |_params: Value, _cancel| async move {
        HandlerResult::Err(JsonRpcError::new(-32602, "Invalid params"))
    });
    let id = RequestId::String("rq".to_owned());
    let result = dispatcher
        .dispatch(&id, "bad", Value::Null)
        .await
        .expect("dispatch ok");
    assert_eq!(err_code(result).expect("handler answered Err"), -32602);
}

#[tokio::test]
async fn re_registering_replaces_handler() {
    let dispatcher = Dispatcher::new();
    dispatcher.register("m", |_params: Value, _cancel| async move {
        HandlerResult::Ok(json!("first"))
    });
    dispatcher.register("m", |_params: Value, _cancel| async move {
        HandlerResult::Ok(json!("second"))
    });
    let id = RequestId::Number(1);
    let result = dispatcher
        .dispatch(&id, "m", Value::Null)
        .await
        .expect("dispatch ok");
    assert_eq!(
        ok_value(result).expect("handler answered Ok"),
        json!("second")
    );
}

#[tokio::test]
async fn cancel_trips_token_for_inflight_request() {
    let dispatcher = Dispatcher::new();
    let started = Arc::new(Notify::new());
    let handler_started = Arc::clone(&started);
    dispatcher.register("slow", move |_params: Value, cancel| {
        let handler_started = Arc::clone(&handler_started);
        async move {
            handler_started.notify_one();
            cancel.cancelled().await;
            HandlerResult::Err(JsonRpcError::new(-32800, "cancelled"))
        }
    });

    let id = RequestId::Number(42);
    let cancel_id = id.clone();
    let dispatcher_clone = dispatcher.clone();
    let handle = tokio::spawn(async move {
        dispatcher_clone
            .dispatch(&cancel_id, "slow", Value::Null)
            .await
    });

    started.notified().await;
    dispatcher.cancel(&id);
    let result = tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("did not time out")
        .expect("join ok")
        .expect("dispatch ok");
    assert_eq!(err_code(result).expect("handler answered Err"), -32800);
}

#[tokio::test]
async fn cancel_unknown_id_is_noop() {
    let dispatcher = Dispatcher::new();
    dispatcher.cancel(&RequestId::Number(999));
}

#[tokio::test]
async fn dispatch_removes_cancellation_on_completion() {
    let dispatcher = Dispatcher::new();
    dispatcher.register("fast", |_params: Value, _cancel| async move {
        HandlerResult::Ok(json!(null))
    });
    let id = RequestId::Number(1);
    let _result = dispatcher
        .dispatch(&id, "fast", Value::Null)
        .await
        .expect("dispatch ok");
    let rendered = format!("{dispatcher:?}");
    assert!(rendered.contains("in_flight: 0"));
}

#[tokio::test]
async fn debug_shows_handler_count() {
    let dispatcher = Dispatcher::new();
    dispatcher.register(
        "a",
        |_p: Value, _c| async move { HandlerResult::Ok(json!(0)) },
    );
    dispatcher.register(
        "b",
        |_p: Value, _c| async move { HandlerResult::Ok(json!(0)) },
    );
    let rendered = format!("{dispatcher:?}");
    assert!(rendered.contains("handlers: 2"));
}

#[tokio::test]
async fn dispatch_error_display_includes_method() {
    let dispatcher = Dispatcher::new();
    let id = RequestId::Number(1);
    let err = dispatcher
        .dispatch(&id, "missing", Value::Null)
        .await
        .expect_err("must reject");
    assert!(format!("{err}").contains("missing"));
}

#[tokio::test]
async fn default_dispatcher_matches_new() {
    let lhs = Dispatcher::default();
    let rhs = Dispatcher::new();
    assert_eq!(format!("{lhs:?}"), format!("{rhs:?}"));
}

#[tokio::test]
async fn handler_result_debug_works() {
    let ok = HandlerResult::Ok(json!(1));
    let err = HandlerResult::Err(JsonRpcError::new(0, "x"));
    assert!(format!("{ok:?}").contains("Ok"));
    assert!(format!("{err:?}").contains("Err"));
}

#[tokio::test]
async fn dispatcher_clone_shares_handlers() {
    let dispatcher = Dispatcher::new();
    dispatcher.register("shared", |_p: Value, _c| async move {
        HandlerResult::Ok(json!("shared!"))
    });
    let clone = dispatcher.clone();
    let id = RequestId::Number(5);
    let result = clone
        .dispatch(&id, "shared", Value::Null)
        .await
        .expect("ok");
    assert_eq!(
        ok_value(result).expect("handler answered Ok"),
        json!("shared!")
    );
}
