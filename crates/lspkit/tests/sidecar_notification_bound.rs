//! [MEM-060-SIDECAR-NOTIFICATIONS]: a consumer that only reads responses keeps
//! the notification receiver alive inside `Inbound`. Unsolicited backend
//! events must not queue up behind it for the life of the server.

use lspkit::sidecar::inbound::{self, NOTIFICATION_CAPACITY};
use lspkit::sidecar::{write_frame, DEFAULT_MAX_FRAME};

/// Notifications sent beyond the buffer.
const OVERFLOW: usize = 64;
/// In-memory pipe size; the writer runs concurrently with the reader.
const PIPE_BYTES: usize = 4096;
/// A notification frame.
const NOTIFICATION: &[u8] = b"n";
/// A response frame.
const RESPONSE: &[u8] = b"r";

#[tokio::test]
async fn unread_notifications_stay_bounded() -> Result<(), String> {
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
    let response = session.responses.recv().await.ok_or("reader ended")??;
    if response.as_slice() != RESPONSE {
        return Err("the response frame was misrouted".to_owned());
    }
    let _writer = flood
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let queued = session.notifications.len();
    if queued > NOTIFICATION_CAPACITY {
        return Err(format!(
            "{queued} unread notifications queued, over the {NOTIFICATION_CAPACITY} bound"
        ));
    }
    Ok(())
}
