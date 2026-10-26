//! Newline-delimited JSON framing for sampler IPC. Implements [PROF-HELPER-PROTOCOL].
//!
//! The elevated sampling helper and the cooperative sample file both carry
//! one compact JSON value per line. Reads are bounded so a peer can never make
//! the language server allocate an unbounded frame.

use std::io::{Error, ErrorKind, Result};

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

/// Maximum accepted newline-delimited frame, including its delimiter.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

/// Write a value as one line of compact JSON followed by `\n`, then flush.
///
/// # Errors
///
/// Returns an error if serialization fails or the underlying writer errors.
pub async fn write_message<W, T>(writer: &mut W, value: &T) -> Result<()>
where
    W: AsyncWriteExt + Unpin,
    T: Serialize + ?Sized,
{
    let json =
        serde_json::to_string(value).map_err(|err| Error::new(ErrorKind::InvalidData, err))?;
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

/// Read one newline-delimited JSON value.
///
/// Returns `Ok(None)` on a clean end-of-stream (the peer closed the socket).
///
/// # Errors
///
/// Returns an error if the line cannot be read or fails to deserialize into `T`.
pub async fn read_message<R, T>(reader: &mut R) -> Result<Option<T>>
where
    R: AsyncBufReadExt + Unpin,
    T: DeserializeOwned,
{
    let Some(frame) = read_bounded_frame(reader).await? else {
        return Ok(None);
    };
    let value =
        serde_json::from_slice(&frame).map_err(|err| Error::new(ErrorKind::InvalidData, err))?;
    Ok(Some(value))
}

/// Read one newline-delimited frame without allocating past the wire limit.
async fn read_bounded_frame<R>(reader: &mut R) -> Result<Option<Vec<u8>>>
where
    R: AsyncBufReadExt + Unpin,
{
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok((!frame.is_empty()).then_some(frame));
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index.saturating_add(1));
        if frame.len().saturating_add(take) > MAX_MESSAGE_BYTES {
            return Err(Error::new(ErrorKind::InvalidData, "wire frame too large"));
        }
        let chunk = available
            .get(..take)
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "invalid wire frame boundary"))?;
        frame.extend_from_slice(chunk);
        reader.consume(take);
        if newline.is_some() {
            return Ok(Some(frame));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{AttachErrorKind, HelperCommand, HelperMessage, StackFrame, ThreadSample};
    use tokio::io::BufReader;

    async fn round_trip<T>(value: &T) -> Result<(String, Option<T>)>
    where
        T: Serialize + DeserializeOwned,
    {
        let mut buf: Vec<u8> = Vec::new();
        write_message(&mut buf, value).await?;
        let wire = String::from_utf8_lossy(&buf).into_owned();
        let mut reader = BufReader::new(buf.as_slice());
        Ok((wire, read_message(&mut reader).await?))
    }

    #[tokio::test]
    async fn round_trips_attach_command() {
        let cmd = HelperCommand::Attach {
            pid: 4242,
            sample_rate: 200,
            include_native: true,
        };
        let (wire, decoded) = round_trip(&cmd).await.expect("round-trip attach command");
        assert!(wire.ends_with('\n'), "framed message must end with newline");
        assert!(
            wire.contains(r#""kind":"Attach""#),
            "commands are kind-tagged: {wire}"
        );
        assert_eq!(decoded, Some(cmd));
    }

    #[tokio::test]
    async fn round_trips_samples_message() {
        let msg = HelperMessage::Samples {
            samples: vec![ThreadSample {
                thread_id: 7,
                thread_name: Some("MainThread".to_owned()),
                active: true,
                frames: vec![StackFrame {
                    name: "hot".to_owned(),
                    file: Some("a.py".to_owned()),
                    line: Some(10),
                    module: None,
                }],
            }],
        };
        let (wire, decoded) = round_trip(&msg).await.expect("round-trip sample message");
        assert!(
            !wire.contains("module"),
            "absent options are omitted: {wire}"
        );
        assert_eq!(decoded, Some(msg));
    }

    #[tokio::test]
    async fn round_trips_failure_message() {
        let msg = HelperMessage::Failure {
            error_kind: AttachErrorKind::PermissionDenied,
            message: "attach failed: Operation not permitted".to_owned(),
        };
        let (wire, decoded) = round_trip(&msg).await.expect("round-trip failure message");
        assert!(
            wire.contains(r#""kind":"Failure""#)
                && wire.contains(r#""error_kind":{"kind":"PermissionDenied"}"#),
            "failure frames carry a tagged kind: {wire}"
        );
        assert_eq!(decoded, Some(msg));
    }

    #[tokio::test]
    async fn read_message_returns_none_on_eof() {
        let empty: &[u8] = b"";
        let mut reader = BufReader::new(empty);
        let decoded: Option<HelperMessage> = read_message(&mut reader)
            .await
            .expect("read clean end-of-stream");
        assert_eq!(decoded, None);
    }

    #[tokio::test]
    async fn read_message_errors_on_garbage() {
        let garbage: &[u8] = b"not json\n";
        let mut reader = BufReader::new(garbage);
        let decoded: Result<Option<HelperMessage>> = read_message(&mut reader).await;
        assert!(
            matches!(&decoded, Err(error) if error.kind() == ErrorKind::InvalidData),
            "malformed JSON must surface InvalidData"
        );
    }

    #[tokio::test]
    async fn read_message_rejects_oversized_frames() {
        let message = "x".repeat(MAX_MESSAGE_BYTES);
        let mut wire = format!(r#"{{"kind":"Stopped","pad":"{message}"}}"#);
        wire.push('\n');
        let mut reader = BufReader::new(wire.as_bytes());
        let result: Result<Option<HelperMessage>> = read_message(&mut reader).await;
        assert!(
            matches!(&result, Err(error) if error.kind() == ErrorKind::InvalidData),
            "oversized frame must return InvalidData"
        );
    }
}
