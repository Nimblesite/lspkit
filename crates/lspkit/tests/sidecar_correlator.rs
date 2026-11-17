//! Coarse end-to-end tests for the request-ID correlator.
//!
//! Drives the public API only — `Correlator::new`, `register`, `resolve`,
//! `pending_count`, and `RequestId::get`.

use lspkit::sidecar::{CorrelationError, Correlator, PendingResponse, RequestId};

/// Allocated IDs are monotonic starting at 1.
#[tokio::test]
async fn allocated_ids_are_monotonic_from_one() {
    let correlator: Correlator<String> = Correlator::new();
    let (first_id, _first_rx) = correlator.register();
    let (second_id, _second_rx) = correlator.register();
    let (third_id, _third_rx) = correlator.register();

    assert_eq!(first_id.get(), 1);
    assert_eq!(second_id.get(), 2);
    assert_eq!(third_id.get(), 3);
}

/// Pending count tracks live registrations and falls back to zero after
/// `resolve`.
#[tokio::test]
async fn pending_count_reflects_registrations() {
    let correlator: Correlator<u32> = Correlator::new();
    assert_eq!(correlator.pending_count(), 0);

    let (id_a, rx_a) = correlator.register();
    let (_id_b, _rx_b) = correlator.register();
    assert_eq!(correlator.pending_count(), 2);

    correlator
        .resolve(id_a, 42)
        .expect("resolve should succeed");
    let value = rx_a.await.expect("receiver should resolve");
    assert_eq!(value, 42);
    assert_eq!(correlator.pending_count(), 1);
}

/// Resolving an unknown ID yields `CorrelationError::UnknownId`.
#[tokio::test]
async fn resolving_unknown_id_errors() {
    let correlator: Correlator<u8> = Correlator::new();
    let (real_id, _real_rx) = correlator.register();
    correlator
        .resolve(real_id, 0)
        .expect("first resolve should succeed");

    // After resolving, the id is gone from the pending map.
    let err = correlator
        .resolve(real_id, 7)
        .expect_err("second resolve should fail");
    match err {
        CorrelationError::UnknownId(raw) => assert_eq!(raw, real_id.get()),
        other => assert!(
            matches!(other, CorrelationError::UnknownId(_)),
            "expected UnknownId, got {other:?}"
        ),
    }
}

/// An abandoned ID is removed immediately, without an unbounded cancellation history.
#[tokio::test]
async fn resolve_against_dropped_receiver_errors() {
    let correlator: Correlator<u32> = Correlator::new();
    let (id, rx) = correlator.register();
    drop(rx);
    assert_eq!(correlator.pending_count(), 0);

    let err = correlator
        .resolve(id, 99)
        .expect_err("resolve should fail when receiver is gone");
    match err {
        CorrelationError::UnknownId(raw) => {
            assert_eq!(raw, id.get());
            assert_eq!(correlator.pending_count(), 0);
        }
        other => assert!(
            matches!(other, CorrelationError::UnknownId(_)),
            "expected UnknownId, got {other:?}"
        ),
    }
}

/// `Default::default` produces an empty, usable correlator.
#[tokio::test]
async fn default_constructor_is_usable() {
    let correlator: Correlator<String> = Correlator::default();
    assert_eq!(correlator.pending_count(), 0);
    let (id, rx) = correlator.register();
    correlator
        .resolve(id, "ok".to_string())
        .expect("resolve should succeed");
    assert_eq!(rx.await.expect("receiver"), "ok");
}

/// Pending receivers must not keep a shut-down correlator's registry alive.
#[tokio::test]
async fn receivers_close_when_the_correlator_owner_is_dropped() {
    let correlator = Correlator::<u32>::new();
    let (_, mut checked) = correlator.register();
    let (_, awaited) = correlator.register();
    assert!(matches!(
        checked.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    drop(correlator);
    assert!(matches!(
        checked.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Closed)
    ));
    assert!(awaited.await.is_err());
    drop(checked);
}

/// Responses returning out of allocation order still match the correct
/// receiver.
#[tokio::test]
async fn out_of_order_responses_match_correct_receivers() {
    let correlator: Correlator<&'static str> = Correlator::new();
    let (id_one, rx_one) = correlator.register();
    let (id_two, rx_two) = correlator.register();
    let (id_three, rx_three) = correlator.register();

    correlator
        .resolve(id_two, "two")
        .expect("resolve two should succeed");
    correlator
        .resolve(id_three, "three")
        .expect("resolve three should succeed");
    correlator
        .resolve(id_one, "one")
        .expect("resolve one should succeed");

    assert_eq!(rx_one.await.expect("rx one"), "one");
    assert_eq!(rx_two.await.expect("rx two"), "two");
    assert_eq!(rx_three.await.expect("rx three"), "three");
}

/// `RequestId` derives expected trait impls.
#[test]
fn request_id_traits_are_derived() {
    let id = RequestId::default_from_get(7);
    let copy = id;
    assert_eq!(id, copy);
    assert_eq!(id.get(), 7);
    assert!(format!("{id:?}").contains('7'));
}

/// Error `Display` strings are stable.
#[test]
fn error_display_strings_are_stable() {
    let unknown = CorrelationError::<()>::UnknownId(11);
    assert_eq!(unknown.to_string(), "unknown request id: 11");

    let already = CorrelationError::AlreadyResolved(12, ());
    assert_eq!(already.to_string(), "request 12 already completed");

    assert!(format!("{unknown:?}").contains("UnknownId"));
    assert!(format!("{already:?}").contains("AlreadyResolved"));
}

/// Debug rendering exposes the pending count.
#[tokio::test]
async fn debug_rendering_shows_pending_count() {
    let correlator: Correlator<u8> = Correlator::new();
    let (_id, _rx) = correlator.register();
    let rendered = format!("{correlator:?}");
    assert!(rendered.contains("Correlator"));
    assert!(rendered.contains("pending"));
    assert!(rendered.contains('1'));
}

// --- helpers --------------------------------------------------------------

/// Tiny extension trait so tests can construct a `RequestId`-like value without
/// reaching into private fields. The public API only hands out `RequestId`s
/// via `register`, so this lets the trait-derivation test stay coarse.
trait RequestIdTestExt {
    fn default_from_get(value: u64) -> Self;
}

impl RequestIdTestExt for RequestId {
    fn default_from_get(value: u64) -> Self {
        // Walk the allocator forward until it hands us the value we want.
        // This stays on the public surface — no internal access.
        let correlator: Correlator<()> = Correlator::new();
        let mut current = correlator.register().0;
        while current.get() < value {
            current = correlator.register().0;
        }
        current
    }
}

/// Callers storing in-flight responses name the future through the sidecar facade. [MEM-FIX-SCOPE]
#[tokio::test]
async fn pending_response_is_nameable_from_the_sidecar_facade() {
    let correlator: Correlator<String> = Correlator::new();
    let (id, mut pending): (RequestId, PendingResponse<String>) = correlator.register();
    assert!(pending.try_recv().is_err());
    correlator
        .resolve(id, "answer".to_owned())
        .expect("resolve the stored registration");
    assert_eq!(pending.await.expect("response delivered"), "answer");
    assert_eq!(correlator.pending_count(), 0);
}

/// A response decoded off the wire carries only the raw number; it rebuilds the id. [SIDECAR-REQUEST-LIFETIME]
#[tokio::test]
async fn raw_wire_ids_rebuild_the_request_id_and_resolve() {
    let correlator: Correlator<&str> = Correlator::new();
    let (first, first_wait) = correlator.register();
    let (second, second_wait) = correlator.register();
    let wire = serde_json::json!({ "id": second.get(), "result": "second" });
    let raw = wire
        .get("id")
        .and_then(serde_json::Value::as_u64)
        .expect("numeric wire id");
    assert_eq!(RequestId::from(raw), second);
    correlator
        .resolve(RequestId::from(raw), "second")
        .expect("rebuilt id resolves");
    correlator
        .resolve(RequestId::from(first.get()), "first")
        .expect("first resolves");
    assert_eq!(second_wait.await.expect("second delivered"), "second");
    assert_eq!(first_wait.await.expect("first delivered"), "first");
    assert!(
        matches!(correlator.resolve(RequestId::from(raw), "again"), Err(CorrelationError::UnknownId(id)) if id == raw)
    );
    assert_eq!(correlator.pending_count(), 0);
}
