//! [MEM-LIFETIME-SIDECAR-NOTIFICATIONS]: a consumer that only reads responses keeps
//! the notification receiver alive inside `Inbound`. Unsolicited backend
//! events must not queue up behind it for the life of the server, and every
//! drop is counted and logged, never silent.

use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use lspkit::sidecar::inbound::{self, NOTIFICATION_CAPACITY};
use lspkit::sidecar::{write_frame, DEFAULT_MAX_FRAME};
use serde_json::Value;

/// Notifications sent beyond the buffer.
const OVERFLOW: usize = 64;
/// In-memory pipe size; the writer runs concurrently with the reader.
const PIPE_BYTES: usize = 4096;
/// A notification frame.
const NOTIFICATION: &[u8] = b"n";
/// A response frame.
const RESPONSE: &[u8] = b"r";
/// The drop totals that log a warning: each power of two up to `OVERFLOW`.
const WARNED_AT: [u64; 7] = [1, 2, 4, 8, 16, 32, 64];

/// Collects the JSON log lines the reader emits.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut log = self
            .0
            .lock()
            .map_err(|poison| std::io::Error::other(poison.to_string()))?;
        log.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The `dropped` and `capacity` fields of one WARN event.
type Warning = (Option<u64>, Option<u64>);

/// The fields of every WARN event in `log`.
fn warnings(log: &Captured) -> Result<Vec<Warning>, String> {
    let bytes = log.0.lock().map_err(|poison| poison.to_string())?.clone();
    serde_json::Deserializer::from_slice(&bytes)
        .into_iter::<Value>()
        .map(|event| event.map_err(|error| error.to_string()))
        .filter(|event| {
            event.as_ref().map_or(true, |e| {
                e.pointer("/level").and_then(Value::as_str) == Some("WARN")
            })
        })
        .map(|event| {
            event.map(|e| {
                let field = |name: &str| e.pointer(name).and_then(Value::as_u64);
                (field("/fields/dropped"), field("/fields/capacity"))
            })
        })
        .collect()
}

#[tokio::test]
async fn unread_notifications_stay_bounded_and_every_drop_is_reported() {
    let log = Captured::default();
    let writer_log = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(move || writer_log.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let (reader, mut writer) = tokio::io::duplex(PIPE_BYTES);
    let mut session = inbound::spawn(reader, DEFAULT_MAX_FRAME, Ok, |frame: &Vec<u8>| {
        frame.as_slice() == NOTIFICATION
    });
    let flood = tokio::spawn(async move {
        for _ in 0..NOTIFICATION_CAPACITY + OVERFLOW {
            write_frame(&mut writer, NOTIFICATION).await?;
        }
        write_frame(&mut writer, RESPONSE).await?;
        Ok::<_, lspkit::sidecar::TransportError>(writer)
    });
    // The response follows every notification, so once it arrives the reader
    // has routed them all.
    let response = session.responses.recv().await.expect("the reader runs");
    assert_eq!(response.expect("a response frame"), RESPONSE);
    let _writer = flood
        .await
        .expect("the flood task joins")
        .expect("every frame is written");
    assert_eq!(session.notifications.len(), NOTIFICATION_CAPACITY);
    assert_eq!(
        session.dropped_notifications.load(Ordering::Relaxed),
        OVERFLOW
    );
    let capacity = u64::try_from(NOTIFICATION_CAPACITY).ok();
    let expected: Vec<_> = WARNED_AT.iter().map(|at| (Some(*at), capacity)).collect();
    assert_eq!(warnings(&log), Ok(expected));
    let first = session.notifications.recv().await;
    assert_eq!(first.as_deref(), Some(NOTIFICATION));
    assert_eq!(session.notifications.len(), NOTIFICATION_CAPACITY - 1);
}
