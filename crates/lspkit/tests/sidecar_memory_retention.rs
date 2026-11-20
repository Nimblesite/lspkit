//! Allocation-lifetime proofs for [MEM-LIFETIME] (docs/specs/lspkit-spec.md).
//! Isolated public-API tests are necessary to observe ownership without RSS noise.

use lspkit::sidecar::Correlator;
use lspkit::sidecar::{inbound, write_frame, DEFAULT_MAX_FRAME};
use std::sync::{Arc, Weak};
use tokio::io::DuplexStream;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;

/// Repeated abandonment must release every allocation, not merely bound one leak.
const REQUESTS: usize = 32;
/// Tokio's oneshot allocates its inline response slot even before a response arrives.
const PAYLOAD_BYTES: usize = 64 * 1024;
/// A single framed probe fits without blocking its writer before reader scheduling.
const PIPE_BYTES: usize = 128;
/// The same real frame is sent and observed on every reader.
const PROBE_FRAME: &[u8] = b"ownership-probe";

#[test]
fn resolved_requests_release_their_channel_allocations() {
    let correlator = Correlator::new();
    for _ in 0..REQUESTS {
        let (id, mut receiver) = correlator.register();
        correlator
            .resolve(id, std::array::from_fn::<_, PAYLOAD_BYTES, _>(|_| 0_u8))
            .expect("resolve live request");
        assert!(receiver
            .try_recv()
            .expect("consume response")
            .into_iter()
            .all(|byte| byte == 0));
        assert_eq!(correlator.pending_count(), 0);
    }
    assert_eq!(correlator.pending_count(), 0);
}

#[test]
fn abandoned_requests_release_their_channel_allocations() {
    let correlator: Correlator<[u8; PAYLOAD_BYTES]> = Correlator::new();
    for _ in 0..REQUESTS {
        let (_, receiver) = correlator.register();
        drop(receiver);
    }
    let retained = correlator.pending_count();
    let retained_payload_bytes = retained * std::mem::size_of::<[u8; PAYLOAD_BYTES]>();
    assert_eq!(retained, 0, "{retained} abandoned oneshot senders still own at least {retained_payload_bytes} bytes of heap response slots; no receivers or request work remain");
}

/// Observe scheduler quiescence, rather than guessing how many yields a task needs.
fn observed_runtime() -> Result<(Runtime, mpsc::UnboundedReceiver<()>), std::io::Error> {
    let (idle, observed) = mpsc::unbounded_channel();
    let runtime = Builder::new_current_thread()
        .enable_all()
        .on_thread_park(move || {
            let _ = idle.send(());
        })
        .build()?;
    Ok((runtime, observed))
}

/// Give a real framed reader a heap-owned decoder and complete its first exchange.
async fn tracked_reader(
) -> Result<(inbound::Inbound<Vec<u8>>, DuplexStream, Weak<Vec<u8>>), Box<dyn std::error::Error>> {
    let allocation = Arc::new(vec![0_u8; PAYLOAD_BYTES]);
    let weak = Arc::downgrade(&allocation);
    let (mut writer, reader) = tokio::io::duplex(PIPE_BYTES);
    let mut inbound = inbound::spawn(
        reader,
        DEFAULT_MAX_FRAME,
        tracked_decoder(allocation),
        |_| false,
    );
    write_frame(&mut writer, PROBE_FRAME).await?;
    assert_eq!(
        inbound.responses.recv().await,
        Some(Ok(PROBE_FRAME.to_vec()))
    );
    Ok((inbound, writer, weak))
}

/// Keep a heap allocation alive exactly as long as the decoder remains owned.
fn tracked_decoder(allocation: Arc<Vec<u8>>) -> impl Fn(Vec<u8>) -> Result<Vec<u8>, String> {
    move |frame| {
        let _payload = std::hint::black_box(&allocation);
        Ok(frame)
    }
}

/// Keep peers open through scheduler quiescence; no EOF can hide an owner-drop leak.
async fn retained_readers(
    idle: &mut mpsc::UnboundedReceiver<()>,
) -> Result<(Vec<Weak<Vec<u8>>>, usize), Box<dyn std::error::Error>> {
    let readers =
        futures_util::future::try_join_all((0..REQUESTS).map(|_| tracked_reader())).await?;
    let (writers, allocations): (Vec<_>, Vec<_>) = readers
        .into_iter()
        .map(|(inbound, writer, allocation)| {
            drop(inbound);
            (writer, allocation)
        })
        .unzip();
    while idle.try_recv().is_ok() {}
    assert_eq!(idle.recv().await, Some(()));
    let retained = allocations.iter().filter(|w| w.strong_count() > 0).count();
    drop(writers);
    Ok((allocations, retained))
}

#[test]
fn dropped_inbound_readers_release_their_decoder_allocations() {
    let (runtime, mut idle) = observed_runtime().expect("current-thread observation runtime");
    let (allocations, retained) = runtime
        .block_on(retained_readers(&mut idle))
        .expect("real framed exchanges");
    drop(runtime);
    assert!(allocations.iter().all(|weak| weak.strong_count() == 0));
    assert_eq!(retained, 0, "{retained} dropped inbound readers retain {PAYLOAD_BYTES}-byte decoder allocations after the executor becomes idle");
}
