//! Handler captures own session data, never the dispatcher that owns the handlers.

use super::{document_query, initialize, work_report, Server};
use crate::analysis::{self, Sources};
use crate::wire::Outbox;
use lspkit_server::pull::PullStore;
use lspkit_server::{HandlerResult, Message};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

/// Only data used by registered handlers; excludes the owning dispatcher.
pub(super) struct HandlerState {
    /// Synchronized sources used by diagnostics and initialization.
    pub(super) sources: Sources,
    /// Memoized diagnostics for this session.
    pub(super) pull: PullStore,
    /// Serialized notifications emitted by the workspace watcher.
    pub(super) outbox: Outbox,
    /// Own the watcher task without letting its callback own the session.
    watcher: Mutex<Option<JoinHandle<()>>>,
}

impl HandlerState {
    /// Watcher callbacks borrow session ownership only while handling a change.
    pub(super) fn watch(self: &Arc<Self>, root: &Path) {
        let owner = Arc::downgrade(self);
        let watcher = analysis::watch_root(&self.sources, root, move || {
            if let Some(state) = owner.upgrade() {
                state.pull.invalidate();
                let event = Message::notification("$/referenceWorkspaceChanged", json!({}));
                if let Err(error) = state.outbox.send(event) {
                    tracing::warn!(%error, "reference watcher notification failed");
                }
            }
        });
        if let Some(watcher) = watcher {
            let mut slot = self
                .watcher
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *slot = Some(watcher);
        }
    }
}

impl Drop for HandlerState {
    fn drop(&mut self) {
        let slot = self
            .watcher
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(watcher) = slot.take() {
            watcher.abort();
        }
    }
}

/// Register handlers without retaining their own registry. [MEM-FIX-SCOPE]
pub(super) fn register_handlers(server: &Server) {
    let state = Arc::new(HandlerState {
        sources: server.sources.clone(),
        pull: server.pull.clone(),
        outbox: server.outbox.clone(),
        watcher: Mutex::new(None),
    });
    let init = Arc::clone(&state);
    server.dispatcher.register("initialize", move |params, _| {
        let init = Arc::clone(&init);
        async move { initialize(&init, params) }
    });
    let documents = Arc::clone(&state);
    server
        .dispatcher
        .register("textDocument/diagnostic", move |params, cancel| {
            document_query(Arc::clone(&documents), params, cancel)
        });
    register_metadata(server, state);
}

/// Metadata handlers share only their required data, with no ownership cycle.
fn register_metadata(server: &Server, state: Arc<HandlerState>) {
    server.dispatcher.register("reference/work", move |_, _| {
        let work = Arc::clone(&state);
        async move { HandlerResult::Ok(work_report(&work)) }
    });
    server
        .dispatcher
        .register("shutdown", |_, _| async { HandlerResult::Ok(Value::Null) });
}

/// Release the editor-owned buffer at the actual close notification boundary.
pub(super) fn close_document(server: &Server, params: Value) {
    let Ok(params) = serde_json::from_value::<lsp_types::DidCloseTextDocumentParams>(params) else {
        tracing::warn!("malformed document close notification ignored");
        return;
    };
    let mut documents = server.sources.documents.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("recovering poisoned source registry to release closed document");
        poisoned.into_inner()
    });
    let _ = documents.remove(params.text_document.uri.as_str());
    drop(documents);
    if let Err(error) = server.pull.clear() {
        tracing::warn!(%error, "closed document memo release failed");
    }
}
