//! Owned ingress registration. [MEM-LIFETIME-QUEUED-REQUEST]

use std::sync::{Arc, Weak};

use super::{Cancellations, RequestControl, RequestId};

/// RAII owner of a request queued with [`CancellationHandle::queue`](super::CancellationHandle::queue).
///
/// Hold it until the request is prepared. Dropping it releases the queued
/// control, unless a prepared request has adopted it, so a request the
/// consumer rejects or abandons before preparing leaves nothing behind.
#[derive(Debug)]
#[must_use = "dropping the guard releases the queued request"]
pub struct QueuedRequest {
    /// The queued request.
    id: RequestId,
    /// The queued control, so a reused ID never releases a newer one.
    control: Weak<RequestControl>,
    /// The dispatcher's registry; weak, so the guard never keeps it alive.
    registry: Weak<Cancellations>,
}

impl QueuedRequest {
    /// Own `control`, queued as `id` in `registry`.
    pub(super) const fn new(
        id: RequestId,
        control: Weak<RequestControl>,
        registry: Weak<Cancellations>,
    ) -> Self {
        Self {
            id,
            control,
            registry,
        }
    }

    /// The queued request's ID.
    #[must_use]
    pub const fn id(&self) -> &RequestId {
        &self.id
    }
}

impl Drop for QueuedRequest {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        let Ok(mut guard) = registry.write() else {
            tracing::warn!("cancellation registry unavailable; queued request not released");
            return;
        };
        let unadopted = guard.get(&self.id).is_some_and(|control| {
            std::ptr::eq(Arc::as_ptr(control), self.control.as_ptr())
                && Arc::strong_count(control) == 1
        });
        if unadopted {
            let _released = guard.remove(&self.id);
        }
    }
}
