//! Real close notifications must release Salsa-owned diagnostics. [LSP-PULL-SALSA]

use super::{handle_notification, Outbox, Server};
use lspkit_server::Message;
use serde_json::json;
use std::sync::Arc;

#[test]
fn closing_documents_releases_their_salsa_diagnostic_allocations() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let server = Server::new(Outbox::new(sender));
    let allocations = (0..32).map(|index| {
        let uri = format!("file:///pull-retention/{index}.json");
        let opened = Message::notification("textDocument/didOpen", json!({
            "textDocument": {"uri": uri, "version": 1, "languageId": "json", "text": "{"}
        }));
        assert!(!handle_notification(&server, &opened));
        let report = server.pull.diagnostics(&uri, 1).expect("real JSON analysis");
        assert_eq!(report.items.len(), 1);
        let allocation = Arc::downgrade(&report.items);
        drop(report);
        let closed = Message::notification("textDocument/didClose", json!({
            "textDocument": {"uri": uri}
        }));
        assert!(!handle_notification(&server, &closed));
        allocation
    }).collect::<Vec<_>>();
    assert_eq!(allocations.iter().map(std::sync::Weak::strong_count).sum::<usize>(), 0,
        "closed documents retain real Salsa diagnostic allocations while the session remains alive");
    assert!(server.sources.documents.lock().expect("source documents").is_empty());
}
