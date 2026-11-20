//! [MEM-LIFETIME]: actual allocator bytes, before any count accessor can sweep entries.

use lspkit::sidecar::{Correlator, RequestId};

#[path = "../../../test-support/state.rs"]
mod state;

/// Repeated abandoned requests expose cumulative heap retention.
const REQUESTS: usize = 32;
/// The channel allocates this response slot inline even when no response arrives.
const RESPONSE_BYTES: usize = 64 * 1024;

/// Warm the map to the peak occupancy so retained bucket capacity is not mistaken for a leak.
fn warmed_correlator() -> Result<Correlator<[u8; RESPONSE_BYTES]>, Box<dyn std::error::Error>> {
    let correlator = Correlator::new();
    let pending: Vec<_> = (0..REQUESTS).map(|_| correlator.register()).collect();
    for (id, mut receiver) in pending {
        resolve_warmup(&correlator, id)?;
        let _response = receiver.try_recv()?;
    }
    Ok(correlator)
}

/// Keep the large response and receive buffers in separate stack frames.
fn resolve_warmup(
    correlator: &Correlator<[u8; RESPONSE_BYTES]>,
    id: RequestId,
) -> Result<(), Box<dyn std::error::Error>> {
    correlator.resolve(id, std::array::from_fn(|_| 0_u8))?;
    Ok(())
}

#[test]
fn abandoned_receivers_free_heap_without_a_later_correlator_call() {
    let correlator = warmed_correlator().expect("completed warm-up exchanges");
    state::assert_releases_heap(|| {
        for _ in 0..REQUESTS {
            let (_, receiver) = correlator.register();
            drop(receiver);
        }
    });
    assert_eq!(correlator.pending_count(), 0);
}
