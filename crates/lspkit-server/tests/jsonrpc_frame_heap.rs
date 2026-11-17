//! [JSONRPC-FRAME-LIMIT] [MEM-FIX-SCOPE] A body shorter than its claimed length
//! costs only the bytes that arrived. Single-test binary: the instrumented
//! allocator sees no sibling tests.

use std::io::{Cursor, ErrorKind};

use lspkit_server::jsonrpc::{read_message, MAX_FRAME_BYTES, MAX_HEADER_BYTES};
use lspkit_server::{FramingError, Message};
use tokio::io::BufReader;

#[path = "../../../test-support/state.rs"]
mod state;

/// The bytes that actually arrive before the peer hangs up.
const SENT: &[u8] = br#"{"jsonrpc":"2.0","method""#;
/// Far above a short read's real cost, far below the claimed body.
const HEAP_BOUND: usize = 64 * 1024;

/// An endless header line: 64 header caps of `A` and no newline.
const ENDLESS_HEADER_BYTES: usize = 64 * 8 * 1024;

/// One read of `raw`, a header line that never ends; the caller owns its bytes.
fn endless_header(runtime: &tokio::runtime::Runtime, raw: &[u8]) -> Result<Message, FramingError> {
    runtime.block_on(read_message(&mut BufReader::new(raw)))
}

/// One read of a frame claiming the largest allowed body but carrying `SENT`.
fn short_read(runtime: &tokio::runtime::Runtime) -> Result<Message, FramingError> {
    let mut raw = format!("Content-Length: {MAX_FRAME_BYTES}\r\n\r\n").into_bytes();
    raw.extend_from_slice(SENT);
    runtime.block_on(read_message(&mut BufReader::new(Cursor::new(raw))))
}

#[test]
fn a_short_body_allocates_what_arrived_not_what_was_claimed() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    let mut outcome = None;
    let stats = state::heap_change(|| outcome = Some(short_read(&runtime)));
    assert!(
        stats.bytes_allocated < HEAP_BOUND,
        "a {}-byte body claiming {MAX_FRAME_BYTES} allocated {} bytes",
        SENT.len(),
        stats.bytes_allocated
    );
    let err = outcome
        .expect("the read ran")
        .expect_err("the body is short");
    assert!(
        matches!(&err, FramingError::Io(io) if io.kind() == ErrorKind::UnexpectedEof),
        "{err:?}"
    );
    state::assert_releases_heap(|| drop(short_read(&runtime)));

    let raw = vec![b'A'; ENDLESS_HEADER_BYTES];
    let mut header = None;
    let stats = state::heap_change(|| header = Some(endless_header(&runtime, &raw)));
    assert!(
        stats.bytes_allocated < HEAP_BOUND,
        "a {ENDLESS_HEADER_BYTES}-byte header line under a {MAX_HEADER_BYTES}-byte cap allocated {} bytes",
        stats.bytes_allocated
    );
    let err = header
        .expect("the read ran")
        .expect_err("the header never ends");
    assert!(matches!(err, FramingError::HeaderTooLarge), "{err:?}");
}
