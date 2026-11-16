//! The initialized reference watcher must not retain closed-session source allocations.

use super::{handle_notification, register_handlers, Outbox, Server};
use lspkit_server::{HandlerResult, Message, RequestId};
use serde_json::json;
use std::sync::Arc;

/// A real source allocation makes retention independent of RSS or allocator caching.
const SOURCE_BYTES: usize = 64 * 1024;

#[tokio::test]
async fn initialized_watcher_releases_session_source_allocations() {
    let root = tempfile::tempdir().expect("real watched workspace");
    let uri = lspkit_server::path_to_uri(root.path()).expect("workspace URI");
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let server = Server::new(Outbox::new(sender));
    register_handlers(&server);
    let result = server
        .dispatcher
        .dispatch(
            &RequestId::Number(1),
            "initialize",
            json!({"capabilities": {}, "workspaceFolders": [{"uri": uri, "name": "memory-proof"}]}),
        )
        .await;
    assert!(matches!(result, Ok(HandlerResult::Ok(_))));
    let open = Message::notification(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": "file:///memory-proof/document.json", "version": 1, "text": " ".repeat(SOURCE_BYTES)
        }}),
    );
    assert!(!handle_notification(&server, &open));
    let documents = Arc::downgrade(&server.sources.documents);
    drop(server);
    assert_eq!(
        documents.strong_count(),
        0,
        "initialized watcher retains the real source buffer after session teardown"
    );
}
