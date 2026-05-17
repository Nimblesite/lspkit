//! Length-prefixed framed transport.
//!
//! Each frame is a 4-byte little-endian length followed by a payload of that
//! many bytes. The payload encoding is the consumer's choice (`MessagePack`,
//! JSON, CBOR) — this module is unaware of the wire format.

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Errors from framed reads and writes.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Underlying I/O failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Frame length exceeded the configured maximum.
    #[error("frame too large: {0} bytes (limit {1})")]
    FrameTooLarge(usize, usize),
}

/// Default maximum frame size (16 MiB) — large enough for realistic IPC,
/// small enough to refuse runaway lengths.
pub const DEFAULT_MAX_FRAME: usize = 16 * 1024 * 1024;

/// Read one frame from `reader`.
///
/// # Errors
/// Returns [`TransportError::Io`] on read failure or
/// [`TransportError::FrameTooLarge`] when the announced length exceeds
/// `max_frame`.
pub async fn read_frame<R>(reader: &mut R, max_frame: usize) -> Result<Vec<u8>, TransportError>
where
    R: AsyncReadExt + Unpin,
{
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes).await?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len > max_frame {
        return Err(TransportError::FrameTooLarge(len, max_frame));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(payload)
}

/// Write one frame to `writer`.
///
/// # Errors
/// Returns [`TransportError::Io`] on write failure or
/// [`TransportError::FrameTooLarge`] when `payload.len()` exceeds `u32::MAX`.
pub async fn write_frame<W>(writer: &mut W, payload: &[u8]) -> Result<(), TransportError>
where
    W: AsyncWriteExt + Unpin,
{
    let len = u32::try_from(payload.len())
        .map_err(|_| TransportError::FrameTooLarge(payload.len(), u32::MAX as usize))?;
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(payload).await?;
    writer.flush().await?;
    Ok(())
}
