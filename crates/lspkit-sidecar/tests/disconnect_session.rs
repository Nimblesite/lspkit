//! Native framed connections distinguish clean disconnects from truncation.
//! Implements [LSP-PULL-TRANSPORT] without an in-memory transport or mock backend.

use std::time::Duration;

use lspkit_sidecar::{inbound, DEFAULT_MAX_FRAME};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedReceiver;

/// A real native connection, with independent client and service endpoints.
async fn native_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind native service");
    let address = listener.local_addr().expect("bound service address");
    let client = TcpStream::connect(address)
        .await
        .expect("connect native client");
    let (service, _) = listener.accept().await.expect("accept native connection");
    (client, service)
}

/// A real local service writes its wire bytes, then closes its native connection.
async fn service_stream(bytes: &[u8]) -> inbound::Inbound<Value> {
    let (client, mut service) = native_pair().await;
    service
        .write_all(bytes)
        .await
        .expect("write native frame bytes");
    service
        .shutdown()
        .await
        .expect("close native service output");
    inbound::spawn(
        client,
        DEFAULT_MAX_FRAME,
        |bytes| decode(&bytes),
        |envelope| envelope.get("method").is_some(),
    )
}

/// Decode the actual public wire payload.
fn decode(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|error| error.to_string())
}

/// Guard a native connection wait without relying on a sleep.
async fn received<T>(receiver: &mut UnboundedReceiver<T>) -> Option<T> {
    tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .expect("native reader settles")
}

/// A corrupt frame produces one visible failure, then releases both channels.
async fn assert_truncation(bytes: &[u8]) {
    let mut stream = service_stream(bytes).await;
    let failure = received(&mut stream.responses)
        .await
        .expect("truncation is an error, not clean EOF");
    assert!(!failure
        .expect_err("a partial frame cannot decode successfully")
        .is_empty());
    assert_closed(stream).await;
}

/// Every terminal disconnect releases channels and reader ownership.
async fn assert_closed(mut stream: inbound::Inbound<Value>) {
    assert_eq!(
        received(&mut stream.responses).await,
        None,
        "failure is terminal"
    );
    assert_eq!(
        received(&mut stream.notifications).await,
        None,
        "notification ownership ends"
    );
    stream.task.await.expect("reader exits without panicking");
}

#[tokio::test]
async fn native_disconnects_preserve_prefix_and_payload_failures_after_valid_traffic() {
    let mut clean = service_stream(&[]).await;
    assert_eq!(
        received(&mut clean.responses).await,
        None,
        "zero-byte EOF is normal"
    );
    assert_eq!(received(&mut clean.notifications).await, None);
    clean.task.await.expect("clean native reader exit");
    assert_truncation(&[4, 0]).await;
    assert_truncation(&[4, 0, 0, 0, b'{']).await;
    valid_then_truncated().await;
}

/// Earlier valid responses remain ordered even when the following frame is torn.
async fn valid_then_truncated() {
    let payload = br#"{"id":7,"result":[]}"#;
    let length = u32::try_from(payload.len())
        .expect("small fixture payload")
        .to_le_bytes();
    let bytes = [length.as_slice(), payload.as_slice(), &[8, 0, 0, 0, b'{']].concat();
    let mut stream = service_stream(&bytes).await;
    assert_eq!(
        received(&mut stream.responses).await,
        Some(Ok(json!({"id":7,"result":[]})))
    );
    let failure = received(&mut stream.responses)
        .await
        .expect("torn second frame remains visible");
    assert!(
        failure.is_err(),
        "valid traffic cannot hide subsequent truncation"
    );
    assert_closed(stream).await;
}
