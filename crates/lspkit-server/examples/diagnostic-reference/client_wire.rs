//! Typed backend envelopes and rendered consumer events. [LSP-PULL-TRANSPORT]

use lspkit::sidecar::inbound::Inbound;
use lspkit_server::RequestId;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::io::AsyncWriteExt;

/// A notification has a method; a correlated response has a required identity.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum Packet {
    /// An unsolicited backend notification.
    Notification {
        /// The notification method name.
        method: String,
        /// Remaining notification fields beyond the method.
        #[serde(flatten)]
        fields: Map<String, Value>,
    },
    /// A correlated backend response.
    Response {
        /// The request identity echoed from the inbound request.
        id: RequestId,
        /// Remaining response fields beyond the identity.
        #[serde(flatten)]
        fields: Map<String, Value>,
    },
}

/// Closure observations come from receiving EOF on each independent channel.
#[derive(Default, Serialize)]
pub(super) struct Closed {
    /// The response channel reached EOF.
    responses_closed: bool,
    /// The notification channel reached EOF.
    notifications_closed: bool,
}

/// A native consumer's user-facing journal, rather than diagnostic logging.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Event {
    /// A rendered unsolicited notification.
    Notification {
        /// The decoded notification envelope.
        envelope: Packet,
    },
    /// A rendered correlated response.
    Response {
        /// The decoded response envelope.
        envelope: Packet,
    },
    /// A decode failure that could not become a typed reply.
    Error {
        /// The human-readable decode failure.
        message: String,
    },
    /// One terminal observation of channel, task and process endings.
    Closed {
        /// Which independent channels reached EOF.
        #[serde(flatten)]
        streams: Closed,
        /// Whether the reader task ended without error.
        reader_ended: bool,
        /// Whether the backend process exited successfully, when observed.
        backend_success: Option<bool>,
    },
}

/// JSON rendering and native stdout failures remain visible.
#[derive(Debug, thiserror::Error)]
pub(super) enum WireError {
    /// A journal event could not be serialized.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Native stdout write or flush failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Decode an actual schema; an uncorrelated error packet cannot become a reply.
pub(super) fn decode(payload: &[u8]) -> Result<Packet, String> {
    serde_json::from_slice(payload).map_err(|error| error.to_string())
}

/// The shared reader delegates protocol classification to its real consumer.
pub(super) fn is_notification(packet: &Packet) -> bool {
    matches!(packet, Packet::Notification { .. })
}

/// Render every ordered response and unsolicited notification until both EOFs.
pub(super) async fn render(
    session: &mut Inbound<Packet>,
    closed: &mut Closed,
) -> Result<(), WireError> {
    while let Some(event) = next_event(session, closed).await {
        emit(&event).await?;
    }
    Ok(())
}

/// Observe the two channels independently, including terminal decode failures.
async fn next_event(session: &mut Inbound<Packet>, closed: &mut Closed) -> Option<Event> {
    while !(closed.responses_closed && closed.notifications_closed) {
        let event = tokio::select! {
            item = session.responses.recv(), if !closed.responses_closed =>
                packet_event(item, &mut closed.responses_closed),
            item = session.notifications.recv(), if !closed.notifications_closed =>
                packet_event(item.map(Ok), &mut closed.notifications_closed),
        };
        if event.is_some() {
            return event;
        }
    }
    None
}

/// The envelope's schema determines the rendered stream identity.
fn packet_event(item: Option<Result<Packet, String>>, closed: &mut bool) -> Option<Event> {
    match item {
        Some(Ok(envelope @ Packet::Notification { .. })) => Some(Event::Notification { envelope }),
        Some(Ok(envelope @ Packet::Response { .. })) => Some(Event::Response { envelope }),
        Some(Err(message)) => Some(Event::Error { message }),
        None => {
            *closed = true;
            None
        }
    }
}

/// Render observed channel, task and process termination exactly once.
pub(super) async fn finish(
    streams: Closed,
    reader_ended: bool,
    backend_success: Option<bool>,
) -> Result<(), WireError> {
    emit(&Event::Closed {
        streams,
        reader_ended,
        backend_success,
    })
    .await
}

/// Emit one structured user-facing event without logging its payload.
pub(super) async fn emit(event: &impl Serialize) -> Result<(), WireError> {
    let mut bytes = serde_json::to_vec(event)?;
    bytes.push(b'\n');
    let mut output = tokio::io::stdout();
    output.write_all(&bytes).await?;
    output.flush().await?;
    Ok(())
}
