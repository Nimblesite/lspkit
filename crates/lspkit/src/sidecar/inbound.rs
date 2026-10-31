//! Read notifications while no request is pending. [LSP-PULL-TRANSPORT]
//!
//! Codec and envelope classification belong to the caller. This layer only
//! separates response flow from unsolicited backend events.

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::sidecar::transport::read_frame;

/// A single reader task with distinct notification and response channels.
#[derive(Debug)]
pub struct Inbound<E> {
    /// Ordered response stream, including a terminal read/codec failure.
    pub responses: mpsc::UnboundedReceiver<Result<E, String>>,
    /// Unsolicited backend notifications, delivered even while idle.
    pub notifications: mpsc::UnboundedReceiver<E>,
    /// Reader ownership, aborted when the owning transport is dropped.
    pub task: JoinHandle<()>,
}

/// Spawn a framed read loop without coupling shared infrastructure to a codec.
pub fn spawn<R, E, D, N>(reader: R, max_frame: usize, decode: D, notification: N) -> Inbound<E>
where
    R: AsyncRead + Send + Unpin + 'static,
    E: Send + 'static,
    D: Fn(Vec<u8>) -> Result<E, String> + Send + 'static,
    N: Fn(&E) -> bool + Send + 'static,
{
    let (responses_tx, responses) = mpsc::unbounded_channel();
    let (notifications_tx, notifications) = mpsc::unbounded_channel();
    let task = tokio::spawn(read_loop(
        reader,
        max_frame,
        decode,
        notification,
        Channels {
            responses: responses_tx,
            notifications: notifications_tx,
        },
    ));
    Inbound {
        responses,
        notifications,
        task,
    }
}

/// Continuously decodes frames until EOF, failure, or channel closure.
async fn read_loop<R, E, D, N>(
    mut reader: R,
    max_frame: usize,
    decode: D,
    notification: N,
    channels: Channels<E>,
) where
    R: AsyncRead + Send + Unpin + 'static,
    E: Send + 'static,
    D: Fn(Vec<u8>) -> Result<E, String> + Send + 'static,
    N: Fn(&E) -> bool + Send + 'static,
{
    loop {
        let frame = next_frame(&mut reader, max_frame).await;
        let decoded = frame.and_then(|frame| frame.map(&decode).transpose());
        if !channels.forward(decoded, &notification) {
            return;
        }
    }
}

/// Groups the destinations so the read loop has one routing responsibility.
struct Channels<E> {
    /// Carries request replies and terminal stream failures in arrival order.
    responses: mpsc::UnboundedSender<Result<E, String>>,
    /// Carries unsolicited envelopes independently of request activity.
    notifications: mpsc::UnboundedSender<E>,
}

impl<E> Channels<E> {
    /// Routes one decoded item and reports whether the reader should continue.
    fn forward(&self, item: Result<Option<E>, String>, notification: &impl Fn(&E) -> bool) -> bool {
        match item {
            Ok(Some(envelope)) if notification(&envelope) => {
                let _ = self.notifications.send(envelope);
                true
            }
            Ok(Some(envelope)) => self.responses.send(Ok(envelope)).is_ok(),
            Err(error) => {
                let _ = self.responses.send(Err(error));
                false
            }
            Ok(None) => false,
        }
    }
}

/// Distinguishes clean EOF from a partial frame without losing the prefix byte.
async fn next_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
    max_frame: usize,
) -> Result<Option<Vec<u8>>, String> {
    let mut first = [0];
    if reader
        .read(&mut first)
        .await
        .map_err(|error| error.to_string())?
        == 0
    {
        return Ok(None);
    }
    let mut prefixed = std::io::Cursor::new(first).chain(reader);
    read_frame(&mut prefixed, max_frame)
        .await
        .map(Some)
        .map_err(|error| error.to_string())
}
