//! [MEM-LIFETIME]: isolate actual reference-server ownership without process-exit masking.

use super::{handle_notification, register_handlers, Outbox, Server};
use lspkit_server::Message;
use serde_json::json;
use std::sync::Arc;

/// Distinct documents model opening and closing files during a long editor session.
const DOCUMENTS: usize = 32;
/// Each document owns a concrete heap buffer, independently of allocator RSS behavior.
const DOCUMENT_BYTES: usize = 64 * 1024;

/// Construct the same session owner used by the native server, without starting IO tasks.
fn server() -> Server {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    Server::new(Outbox::new(sender))
}

/// Drive the real notification handler with one complete source buffer.
fn open_document(server: &Server, index: usize) -> String {
    let uri = format!("file:///memory-audit/{index}.json");
    let message = Message::notification(
        "textDocument/didOpen",
        json!({
            "textDocument": { "uri": uri, "version": 1, "languageId": "json",
                "text": " ".repeat(DOCUMENT_BYTES) }
        }),
    );
    assert!(!handle_notification(server, &message));
    uri
}

#[test]
fn unregistered_session_releases_its_source_allocations() {
    let server = server();
    let _uri = open_document(&server, 0);
    let documents = Arc::downgrade(&server.sources.documents);
    drop(server);
    assert_eq!(documents.strong_count(), 0);
}

#[test]
fn registered_session_releases_its_source_allocations() {
    let server = server();
    let _uri = open_document(&server, 0);
    register_handlers(&server);
    let documents = Arc::downgrade(&server.sources.documents);
    drop(server);
    assert_eq!(
        documents.strong_count(),
        0,
        "handler closures retain the source buffer after every external Server owner is dropped"
    );
}

#[test]
fn closed_documents_release_their_source_buffers() {
    let server = server();
    for index in 0..DOCUMENTS {
        let uri = open_document(&server, index);
        let close = Message::notification(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        );
        assert!(!handle_notification(&server, &close));
    }
    let sources = &server.sources.documents;
    let documents = sources.lock().expect("source allocation registry");
    let retained_bytes: usize = documents.values().map(|document| document.text.len()).sum();
    assert_eq!(
        (documents.len(), retained_bytes),
        (0, 0),
        "didClose retained complete document text buffers after all {DOCUMENTS} documents closed"
    );
}
