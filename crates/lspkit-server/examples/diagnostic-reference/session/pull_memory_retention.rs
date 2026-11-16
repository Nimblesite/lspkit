//! Real close notifications must release Salsa-owned diagnostics. [LSP-PULL-SALSA]

use super::{handle_notification, Outbox, Server};
use lspkit_server::Message;
use serde_json::json;
use std::sync::Arc;

#[test]
fn closing_after_a_source_lock_panic_releases_retained_text() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let server = Server::new(Outbox::new(sender));
    let poisoned = std::panic::catch_unwind(|| {
        let mut documents = server.sources.documents.lock().expect("source registry");
        let _ = documents.insert(
            "file:///poisoned.json".to_owned(),
            crate::analysis::Document {
                text: " ".repeat(64 * 1024),
                version: 1,
            },
        );
        std::panic::resume_unwind(Box::new(()));
    });
    assert!(poisoned.is_err());
    let closed = Message::notification(
        "textDocument/didClose",
        json!({
            "textDocument": {"uri": "file:///poisoned.json"}
        }),
    );
    assert!(!handle_notification(&server, &closed));
    let documents = server
        .sources
        .documents
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        documents
            .values()
            .map(|document| document.text.len())
            .sum::<usize>(),
        0,
        "poisoned close retains a 64 KiB source allocation"
    );
}

#[test]
fn closing_documents_releases_their_salsa_diagnostic_allocations() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let server = Server::new(Outbox::new(sender));
    let allocations = (0..32)
        .map(|index| {
            let uri = format!("file:///pull-retention/{index}.json");
            let opened = Message::notification(
                "textDocument/didOpen",
                json!({
                    "textDocument": {"uri": uri, "version": 1, "languageId": "json", "text": "{"}
                }),
            );
            assert!(!handle_notification(&server, &opened));
            let report = server
                .pull
                .diagnostics(&uri, 1)
                .expect("real JSON analysis");
            assert_eq!(report.items.len(), 1);
            let allocation = Arc::downgrade(&report.items);
            drop(report);
            let closed = Message::notification(
                "textDocument/didClose",
                json!({
                    "textDocument": {"uri": uri}
                }),
            );
            assert!(!handle_notification(&server, &closed));
            allocation
        })
        .collect::<Vec<_>>();
    assert_eq!(
        allocations
            .iter()
            .map(std::sync::Weak::strong_count)
            .sum::<usize>(),
        0,
        "closed documents retain real Salsa diagnostic allocations while the session remains alive"
    );
    assert!(server
        .sources
        .documents
        .lock()
        .expect("source documents")
        .is_empty());
}
