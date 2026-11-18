//! [MEM-060-PULL-ADMIT]: a pull admits a Salsa input and memo for its URI.
//! Once the URI is neither open nor a JSON file in the watched root, nothing
//! will close it, so the session must release it itself.

use std::path::Path;
use std::sync::{Arc, Weak};

use super::{handle_notification, register_handlers, Outbox, Server};
use lspkit_server::{HandlerResult, Message, RequestId};
use serde_json::json;

/// A JSON file with one syntax error, so its memo holds a diagnostic.
const BROKEN: &str = "{";
/// LSP `FileChangeType.Deleted`.
const DELETED: u8 = 3;

/// A session owner with its real handlers, without IO tasks.
fn server() -> Server {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let server = Server::new(Outbox::new(sender));
    register_handlers(&server);
    server
}

/// Write a broken JSON file and return its URI.
fn broken_file(dir: &Path, name: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, BROKEN).expect("write the fixture file");
    lspkit_server::path_to_uri(&path).expect("fixture URI")
}

/// Memoize `uri`'s diagnostics; returns a weak view of the memo.
fn memoize(server: &Server, uri: &str) -> Weak<Vec<lsp_types::Diagnostic>> {
    let report = server.pull.diagnostics(uri, 0).expect("real JSON analysis");
    assert_eq!(report.items.len(), 1, "the broken file has one diagnostic");
    let memo = Arc::downgrade(&report.items);
    drop(report);
    assert_eq!(memo.strong_count(), 1, "the store memoizes the report");
    memo
}

#[tokio::test]
async fn a_pull_for_an_untracked_document_releases_its_memo() {
    let outside = tempfile::tempdir().expect("fixture directory");
    let server = server();
    let uri = broken_file(outside.path(), "outside.json");
    let memo = memoize(&server, &uri);
    let reply = server
        .dispatcher
        .dispatch(
            &RequestId::Number(1),
            "textDocument/diagnostic",
            json!({"textDocument": {"uri": uri}}),
        )
        .await;
    assert!(matches!(reply, Ok(HandlerResult::Ok(_))), "{reply:?}");
    assert_eq!(
        memo.strong_count(),
        0,
        "a pull for a document that is neither open nor in the workspace keeps its memo"
    );
}

#[tokio::test]
async fn a_deleted_workspace_file_releases_its_memo() {
    let root = tempfile::tempdir().expect("workspace root");
    let server = server();
    *server.sources.root.lock().expect("root slot") = Some(root.path().to_path_buf());
    let uri = broken_file(root.path(), "deleted.json");
    let memo = memoize(&server, &uri);
    std::fs::remove_file(root.path().join("deleted.json")).expect("delete the file");
    let deleted = Message::notification(
        "workspace/didChangeWatchedFiles",
        json!({"changes": [{"uri": uri, "type": DELETED}]}),
    );
    assert!(!handle_notification(&server, &deleted));
    assert_eq!(
        memo.strong_count(),
        0,
        "a deleted workspace file keeps its memo after the editor reported the deletion"
    );
}
