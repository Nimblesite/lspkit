//! Read notifications while no request is pending. [LSP-PULL-TRANSPORT]
//!
//! Codec and envelope classification belong to the caller. This layer only
//! separates response flow from unsolicited backend events.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::sidecar::transport::read_frame;

/// Notifications buffered for a consumer that is not reading them; later ones
/// are dropped and counted. [MEM-060-SIDECAR-NOTIFICATIONS]
pub const NOTIFICATION_CAPACITY: usize = 1024;

/// A single reader task with distinct notification and response channels.
#[derive(Debug)]
pub struct Inbound<E> {
    /// Ordered response stream, including a terminal read/codec failure.
    pub responses: mpsc::UnboundedReceiver<Result<E, String>>,
    /// Unsolicited backend notifications, delivered even while idle. At most
    /// [`NOTIFICATION_CAPACITY`] wait unread.
    pub notifications: mpsc::Receiver<E>,
    /// Notifications dropped because [`NOTIFICATION_CAPACITY`] were unread.
    pub dropped_notifications: Arc<AtomicUsize>,
    /// Reader completion; dropping both receivers stops the reader even while idle.
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
    let (notifications_tx, notifications) = mpsc::channel(NOTIFICATION_CAPACITY);
    let dropped_notifications = Arc::new(AtomicUsize::new(0));
    let task = tokio::spawn(read_loop(
        reader,
        max_frame,
        decode,
        notification,
        Channels {
            responses: responses_tx,
            notifications: notifications_tx,
            dropped: Arc::clone(&dropped_notifications),
        },
    ));
    Inbound {
        responses,
        notifications,
        dropped_notifications,
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
        let frame = tokio::select! {
            biased;
            () = channels.closed() => return,
            frame = next_frame(&mut reader, max_frame) => frame,
        };
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
    notifications: mpsc::Sender<E>,
    /// Counts notifications dropped on a full buffer.
    dropped: Arc<AtomicUsize>,
}

impl<E> Channels<E> {
    /// Either stream may be consumed independently; stop when neither has an owner.
    async fn closed(&self) {
        self.responses.closed().await;
        self.notifications.closed().await;
    }

    /// Routes one decoded item and reports whether the reader should continue.
    fn forward(&self, item: Result<Option<E>, String>, notification: &impl Fn(&E) -> bool) -> bool {
        match item {
            Ok(Some(envelope)) if notification(&envelope) => {
                self.notify(envelope);
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

impl<E> Channels<E> {
    /// Deliver a notification without blocking the reader: a reader blocked on
    /// a consumer that waits only for responses would deadlock. A full buffer
    /// drops the notification. [MEM-060-SIDECAR-NOTIFICATIONS]
    fn notify(&self, envelope: E) {
        if let Err(mpsc::error::TrySendError::Full(_dropped)) =
            self.notifications.try_send(envelope)
        {
            let _ = self.dropped.fetch_add(1, Ordering::Relaxed);
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
