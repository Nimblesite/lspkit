//! Coarse end-to-end tests for the framed-IPC transport layer.
//!
//! Round-trips frames over an in-memory `tokio::io::duplex` pipe to exercise
//! the public `read_frame` / `write_frame` API without reaching into
//! internals.

use lspkit::sidecar::{read_frame, write_frame, TransportError, DEFAULT_MAX_FRAME};
use tokio::io::{duplex, AsyncWriteExt};

/// Default-max constant matches the documented 16 MiB ceiling.
#[test]
fn default_max_frame_is_sixteen_mib() {
    assert_eq!(DEFAULT_MAX_FRAME, 16 * 1024 * 1024);
}

/// Round-trip a non-empty payload through the framed transport.
#[tokio::test]
async fn round_trip_simple_payload() {
    let (mut client, mut server) = duplex(64);
    let payload = b"hello sidecar".to_vec();

    write_frame(&mut client, &payload)
        .await
        .expect("write should succeed");
    let received = read_frame(&mut server, DEFAULT_MAX_FRAME)
        .await
        .expect("read should succeed");

    assert_eq!(received, payload);
}

/// Empty payload is a valid frame: 4-byte length of zero, no body.
#[tokio::test]
async fn round_trip_empty_payload() {
    let (mut client, mut server) = duplex(16);

    write_frame(&mut client, &[])
        .await
        .expect("empty write should succeed");
    let received = read_frame(&mut server, DEFAULT_MAX_FRAME)
        .await
        .expect("empty read should succeed");

    assert_eq!(received, Vec::<u8>::new());
}

/// Multiple sequential frames decode in order.
#[tokio::test]
async fn multiple_frames_preserve_order() {
    let (mut client, mut server) = duplex(256);
    let frames: Vec<Vec<u8>> = vec![b"alpha".to_vec(), b"beta".to_vec(), b"gamma".to_vec()];

    for payload in &frames {
        write_frame(&mut client, payload)
            .await
            .expect("write should succeed");
    }
    drop(client);

    let mut decoded: Vec<Vec<u8>> = Vec::new();
    for _ in 0..frames.len() {
        let frame = read_frame(&mut server, DEFAULT_MAX_FRAME)
            .await
            .expect("read should succeed");
        decoded.push(frame);
    }
    assert_eq!(decoded, frames);
}

/// A payload at the configured max boundary still passes.
#[tokio::test]
async fn payload_at_max_boundary_passes() {
    let (mut client, mut server) = duplex(4096);
    let max_frame = 1024_usize;
    let payload = vec![0xABu8; max_frame];

    let writer = tokio::spawn(async move {
        write_frame(&mut client, &payload)
            .await
            .expect("write should succeed");
        payload
    });

    let received = read_frame(&mut server, max_frame)
        .await
        .expect("read should succeed");
    let sent = writer.await.expect("writer task should join");
    assert_eq!(received, sent);
}

/// A frame announcing a length above `max_frame` is rejected and the byte
/// stream is left positioned just past the length prefix.
#[tokio::test]
async fn oversized_announced_length_is_rejected() {
    let (mut client, mut server) = duplex(64);
    let announced: u32 = 1024;
    client
        .write_all(&announced.to_le_bytes())
        .await
        .expect("len write should succeed");
    client.flush().await.expect("flush should succeed");

    let err = read_frame(&mut server, 16)
        .await
        .expect_err("oversized frame must error");
    let observed = usize::try_from(announced).expect("announced length fits in usize");
    assert!(
        matches!(err, TransportError::FrameTooLarge(len, 16) if len == observed),
        "expected FrameTooLarge({observed}, 16), got {err:?}"
    );
}

/// A payload whose length cannot fit the u32 prefix is refused before any byte is written.
/// The zeroed allocation is only reserved, never touched, so the test stays cheap.
#[tokio::test]
async fn unframeable_payload_is_refused_before_writing() {
    let limit = usize::try_from(u32::MAX).expect("u32 fits usize on supported targets");
    let payload = vec![0_u8; limit + 1];
    let mut sink: Vec<u8> = Vec::new();
    let err = write_frame(&mut sink, &payload)
        .await
        .expect_err("a payload past u32::MAX must be refused");
    assert!(
        matches!(err, TransportError::FrameTooLarge(len, max) if len == limit + 1 && max == limit),
        "expected FrameTooLarge({}, {limit}), got {err:?}",
        limit + 1
    );
    assert_eq!(sink, Vec::<u8>::new(), "nothing reaches the writer");
}

/// An EOF before the 4-byte length prefix surfaces as an I/O error.
#[tokio::test]
async fn eof_before_length_prefix_yields_io_error() {
    let (client, mut server) = duplex(16);
    drop(client);

    let err = read_frame(&mut server, DEFAULT_MAX_FRAME)
        .await
        .expect_err("clean EOF before length should error");
    assert!(matches!(err, TransportError::Io(_)));
}

/// A truncated payload (length prefix announces more bytes than are written)
/// surfaces as an I/O error.
#[tokio::test]
async fn truncated_payload_yields_io_error() {
    let (mut client, mut server) = duplex(64);
    let announced: u32 = 8;
    client
        .write_all(&announced.to_le_bytes())
        .await
        .expect("len write should succeed");
    client
        .write_all(&[1u8, 2, 3])
        .await
        .expect("partial payload write should succeed");
    client.flush().await.expect("flush should succeed");
    drop(client);

    let err = read_frame(&mut server, DEFAULT_MAX_FRAME)
        .await
        .expect_err("truncated frame must error");
    assert!(matches!(err, TransportError::Io(_)));
}

/// Error `Display` impls render the documented strings.
#[test]
fn error_display_strings_are_stable() {
    let too_large = TransportError::FrameTooLarge(100, 50);
    assert_eq!(
        too_large.to_string(),
        "frame too large: 100 bytes (limit 50)"
    );

    let io_err: TransportError =
        std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "boom").into();
    assert_eq!(io_err.to_string(), "boom");
    assert!(format!("{io_err:?}").contains("Io"));
}

/// Writes that fail mid-frame surface as I/O errors.
#[tokio::test]
async fn write_after_reader_drop_yields_io_error() {
    let (mut client, server) = duplex(4);
    drop(server);

    let payload = vec![0u8; 1024];
    let err = write_frame(&mut client, &payload)
        .await
        .expect_err("write to closed reader must error");
    assert!(matches!(err, TransportError::Io(_)));
}
