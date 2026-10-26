//! Serialized message egress for the LSP session. [LSP-PULL-CANCELLATION]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use lspkit_server::{Message, MessageWriter};
use tokio::io::Stdout;
use tokio::sync::mpsc;
use tokio::sync::Notify;

/// Egress failed because the writer pump has stopped.
#[derive(Debug, thiserror::Error)]
#[error("reference egress closed")]
pub(super) struct EgressClosed;

/// Serialized message egress: one pump writes frames in send order, and the
/// session tracks outstanding requests so exit never strands a reply.
#[derive(Clone)]
pub(super) struct Outbox {
    inbox: mpsc::UnboundedSender<Message>,
    sent: Arc<AtomicUsize>,
    written: Arc<AtomicUsize>,
    pending: Arc<AtomicUsize>,
    idle: Arc<Notify>,
}

impl Outbox {
    /// Wrap the queue feeding the writer pump.
    pub(super) fn new(inbox: mpsc::UnboundedSender<Message>) -> Self {
        Self {
            inbox,
            sent: Arc::new(AtomicUsize::new(0)),
            written: Arc::new(AtomicUsize::new(0)),
            pending: Arc::new(AtomicUsize::new(0)),
            idle: Arc::new(Notify::new()),
        }
    }

    /// Queue `message`; frames leave in exactly this order.
    pub(super) fn send(&self, message: Message) -> Result<(), EgressClosed> {
        let _ = self.sent.fetch_add(1, Ordering::AcqRel);
        self.inbox.send(message).map_err(|_| EgressClosed)
    }

    /// Mark one spawned request as awaiting its terminal reply.
    pub(super) fn begin_request(&self) {
        let _ = self.pending.fetch_add(1, Ordering::AcqRel);
    }

    /// Record a answered request, waking the session once all are done.
    pub(super) fn finish_request(&self) {
        if self.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.idle.notify_one();
        }
    }

    /// Record one flushed frame, waking any `flush` waiter.
    fn record_write(&self) {
        let _ = self.written.fetch_add(1, Ordering::AcqRel);
        self.idle.notify_one();
    }

    /// Wait until every queued message is on the wire.
    async fn flush(&self) {
        while self.written.load(Ordering::Acquire) < self.sent.load(Ordering::Acquire) {
            if tokio::time::timeout(Duration::from_secs(5), self.idle.notified())
                .await
                .is_err()
            {
                return;
            }
        }
    }

    /// Wait until every spawned request answered and every queued frame is
    /// on the wire, so `exit`/EOF never strands a reply.
    pub(super) async fn quiesce(&self) {
        while self.pending.load(Ordering::Acquire) > 0 {
            if tokio::time::timeout(Duration::from_secs(5), self.idle.notified())
                .await
                .is_err()
            {
                break;
            }
        }
        self.flush().await;
    }
}

/// Write every queued frame to stdout in send order.
pub(super) async fn pump(
    mut inbox: mpsc::UnboundedReceiver<Message>,
    writer: MessageWriter<Stdout>,
    outbox: Outbox,
) {
    while let Some(message) = inbox.recv().await {
        if writer.write_message(&message).await.is_err() {
            tracing::error!("reference stdout closed");
            return;
        }
        outbox.record_write();
    }
}
