//! Ingress cancellation independent of foreground work. [LSP-PULL-CANCELLATION]
use std::sync::{Arc, Weak};

use serde_json::Value;

use super::{Cancellations, DispatchError, RequestControl, RequestId, RequestInterest};

/// Weak access to the dispatcher's sole registry; does not retain the server or its handlers.
#[derive(Clone, Debug)]
pub struct CancellationHandle {
    registry: Weak<Cancellations>,
}

impl CancellationHandle {
    pub(super) fn new(registry: &Arc<Cancellations>) -> Self {
        Self {
            registry: Arc::downgrade(registry),
        }
    }

    /// Register protocol interest before forwarding to a possibly occupied foreground loop.
    /// `Dispatcher::prepare_call` adopts this same control rather than replacing it.
    /// Completed controls are released by the prepared future; queued controls live with
    /// the dispatcher. An exited owner is harmless. [LSP-PULL-CANCELLATION]
    ///
    /// # Errors
    /// Returns a registration error if the registry is unavailable.
    pub fn register(
        &self,
        id: RequestId,
        interest: Option<RequestInterest>,
    ) -> Result<(), DispatchError> {
        let Some(registry) = self.registry.upgrade() else {
            return Ok(());
        };
        let mut guard = registry
            .write()
            .map_err(|_| DispatchError::RegistrationPoisoned(id.clone()))?;
        register(&mut guard, id, interest);
        Ok(())
    }

    /// Cancel an already registered request from a protocol-reader thread.
    /// Unknown and completed IDs, and an owner that has exited, are harmless.
    pub fn cancel(&self, id: &RequestId) {
        self.with_registry(|registry| {
            if let Some(control) = registry.get(id) {
                control.cancel();
            }
        });
    }

    /// Supersede work whose actual input changed, even if the next pull repeats a result ID.
    /// The consumer supplies its canonical resource; no parallel input/result store is kept.
    /// The first interruption still wins. [LSP-PULL-CANCELLATION]
    pub fn supersede(&self, method: &str, resource: &str) {
        self.with_registry(|registry| {
            registry
                .values()
                .filter(|control| {
                    control
                        .interest
                        .as_ref()
                        .is_some_and(|interest| interest.matches(method, resource))
                })
                .for_each(|control| control.supersede());
        });
    }

    /// A replaced consumer context retires all its work through the same registry.
    /// Registrations retain the first interruption and their own completion guard.
    /// [LSP-PULL-CANCELLATION]
    pub fn supersede_all(&self) {
        self.with_registry(|registry| {
            for control in registry.values() {
                control.supersede();
            }
        });
    }

    /// Protocol controls share one registry guard and one observable failure path.
    fn with_registry(
        &self,
        action: impl FnOnce(&std::collections::HashMap<RequestId, Arc<RequestControl>>),
    ) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        match registry.read() {
            Ok(guard) => action(&guard),
            Err(error) => tracing::warn!(%error, "Request cancellation registry unavailable"),
        };
    }

    /// Decode standard `$/cancelRequest` parameters without application-specific routing.
    pub fn cancel_request(&self, params: &Value) {
        if let Some(id) = params
            .get("id")
            .and_then(|id| serde_json::from_value(id.clone()).ok())
        {
            self.cancel(&id);
        }
    }
}

/// Preparing later adopts this control, including any interruption received while queued.
fn register(
    registry: &mut std::collections::HashMap<RequestId, Arc<RequestControl>>,
    id: RequestId,
    interest: Option<RequestInterest>,
) {
    if registry.contains_key(&id) {
        return;
    }
    supersede(registry, interest.as_ref());
    registry.insert(
        id,
        Arc::new(RequestControl {
            interest,
            ..RequestControl::default()
        }),
    );
}

/// Replacement is limited to a different input for the same method and resource.
fn supersede(
    registry: &std::collections::HashMap<RequestId, Arc<RequestControl>>,
    interest: Option<&RequestInterest>,
) {
    if let Some(interest) = interest {
        for previous in registry.values().filter(|control| {
            control
                .interest
                .as_ref()
                .is_some_and(|previous| interest.supersedes(previous))
        }) {
            previous.supersede();
        }
    }
}
