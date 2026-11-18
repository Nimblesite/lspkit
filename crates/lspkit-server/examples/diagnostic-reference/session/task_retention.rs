//! [MEM-060-EXAMPLE-TASKS]: a request still in flight when the session ends
//! must not keep the session's documents alive.

use std::sync::Arc;

use super::{handle_notification, schedule, Outbox, Server, Tasks};
use lspkit_server::{HandlerResult, Message, RequestId};
use serde_json::{json, Value};

/// A request whose handler never answers.
const STUCK: &str = "reference/stuck";

/// A session owner without IO tasks.
fn server() -> Server {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    Server::new(Outbox::new(sender))
}

#[tokio::test]
async fn a_stuck_request_does_not_pin_the_ended_session() {
    let server = server();
    let open = Message::notification(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": "file:///stuck.json", "version": 1,
            "languageId": "json", "text": "{}"}}),
    );
    assert!(!handle_notification(&server, &open));
    server.dispatcher.register(STUCK, |_, _| async {
        std::future::pending::<HandlerResult>().await
    });
    let documents = Arc::downgrade(&server.sources.documents);
    let idle = documents.strong_count();
    let mut tasks = Tasks::default();
    schedule(
        &server,
        RequestId::Number(1),
        STUCK,
        Value::Null,
        &mut tasks,
    );
    tokio::task::yield_now().await;
    assert_eq!(
        documents.strong_count(),
        idle + 1,
        "the request is in flight and holds one Server clone"
    );
    drop(server);
    drop(tasks);
    tokio::task::yield_now().await;
    assert_eq!(
        documents.strong_count(),
        0,
        "the stuck request task still holds a Server clone after its session ended"
    );
}
