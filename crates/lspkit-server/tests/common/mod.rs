//! Shared harness for the real `diagnostic-reference` LSP sessions.
//!
//! Every helper here drives the real compiled example binary as an OS child
//! over framed stdio with real files on disk — no fakes, no mocks. Each
//! suite shares one integration binary so every helper has a real caller.

use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use lspkit_server::jsonrpc::read_message;
use lspkit_server::{path_to_uri, FramingError, JsonRpcError, Message, MessageWriter, RequestId};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::time::timeout;

/// Generous guard for pipe waits; never a timing dependency.
pub use reference_program::READ_GUARD;

#[path = "../../../../test-support/reference_program.rs"]
mod reference_program;

pub mod held_source;

/// The next meaningful traffic for an in-flight streamed request.
pub enum Streamed {
    /// One `$/progress` partial carrying the request's token.
    Partial(Value),
    /// The terminal response for the request id.
    Terminal(Message),
}

impl Streamed {
    /// A real partial is required at the in-flight scan barrier.
    pub fn partial(self) -> Option<Value> {
        match self {
            Self::Partial(value) => Some(value),
            Self::Terminal(_) => None,
        }
    }
}

/// Files in a bulk workspace fixture; concurrency uses native pipe barriers.
pub const SCAN_FILES: usize = 64;

/// One framed stdio client of the reference server.
pub struct ReferenceClient {
    child: Child,
    writer: Option<ChildStdin>,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

impl ReferenceClient {
    /// Spawn the real example binary with piped stdio.
    pub fn spawn() -> Self {
        let mut child =
            reference_program::spawn_reference(&[]).expect("spawn diagnostic-reference");
        let stdin = child.stdin.take().expect("stdin pipe");
        let stdout = child.stdout.take().expect("stdout pipe");
        Self {
            child,
            writer: Some(stdin),
            reader: BufReader::new(stdout),
            next_id: 1,
        }
    }

    /// Send a request and return its id.
    pub async fn send(&mut self, method: &str, params: Value) -> RequestId {
        let id = self.take_id();
        self.write(Message::request(id.clone(), method, params))
            .await;
        id
    }

    /// Allocate the next request identity once for ordinary or batched traffic.
    fn take_id(&mut self) -> RequestId {
        let id = RequestId::Number(self.next_id);
        self.next_id += 1;
        id
    }

    /// Put the request and cancellation in one native write, before either is read.
    pub async fn send_cancelled(&mut self, method: &str, params: Value) -> RequestId {
        let id = self.take_id();
        self.write_batch(&[
            Message::request(id.clone(), method, params),
            Message::notification("$/cancelRequest", json!({ "id": id })),
        ])
        .await;
        id
    }

    /// Send a request reusing an explicit id; ids stay usable after a reply.
    pub async fn send_reusing(&mut self, id: &RequestId, method: &str, params: Value) {
        if let RequestId::Number(number) = id {
            self.next_id = self.next_id.max(*number + 1);
        }
        self.write(Message::request(id.clone(), method, params))
            .await;
    }

    /// Send a notification (no response expected).
    pub async fn notify(&mut self, method: &str, params: Value) {
        self.write(Message::notification(method, params)).await;
    }

    /// Write one frame through the open stdin pipe.
    async fn write(&mut self, message: Message) {
        MessageWriter::new(self.writer.as_mut().expect("stdin open"))
            .write_message(&message)
            .await
            .expect("write framed message");
    }

    /// Encode with the shared writer and send the complete native batch once.
    pub async fn write_batch(&mut self, messages: &[Message]) {
        let mut bytes = Vec::new();
        {
            let writer = MessageWriter::new(&mut bytes);
            for message in messages {
                writer
                    .write_message(message)
                    .await
                    .expect("encode native frames");
            }
        }
        let input = self.writer.as_mut().expect("stdin open");
        input.write_all(&bytes).await.expect("write native batch");
        input.flush().await.expect("flush native batch");
    }

    /// Read the response for `id`, skipping unrelated traffic.
    pub async fn response(&mut self, id: &RequestId) -> Message {
        loop {
            let message = self.next_message().await;
            if message.id.as_ref() == Some(id) {
                return message;
            }
        }
    }

    /// Issue a request; return its response plus every `$/progress`
    /// notification carrying `token` that arrived before it.
    pub async fn request_with_progress(
        &mut self,
        method: &str,
        params: Value,
        token: &Value,
    ) -> (Message, Vec<Value>) {
        let id = self.send(method, params).await;
        self.collect_stream(&id, token).await
    }

    /// Drain an already-started real request without losing its partial reports.
    pub async fn collect_stream(&mut self, id: &RequestId, token: &Value) -> (Message, Vec<Value>) {
        let mut partials = Vec::new();
        loop {
            match self.next_streamed(id, token).await {
                Streamed::Partial(value) => partials.push(value),
                Streamed::Terminal(message) => return (message, partials),
            }
        }
    }

    /// Start the same standard workspace scan through the actual public method.
    pub async fn start_scan(&mut self, token: &Value) -> RequestId {
        self.send(
            "workspace/diagnostic",
            json!({
                "previousResultIds": [], "partialResultToken": token
            }),
        )
        .await
    }

    /// A partial proves that the real scan is active and identifies its source.
    pub async fn first_partial(&mut self, id: &RequestId, token: &Value) -> Value {
        let value = self
            .next_streamed(id, token)
            .await
            .partial()
            .expect("scan is in flight");
        assert!(value[0]["uri"].is_string(), "partial carries its document");
        value
    }

    /// Next meaningful traffic for one streamed request: the next `$/progress`
    /// partial carrying `token`, or the terminal response for `id`.
    pub async fn next_streamed(&mut self, id: &RequestId, token: &Value) -> Streamed {
        loop {
            let message = self.next_message().await;
            if message.id.as_ref() == Some(id) {
                return Streamed::Terminal(message);
            }
            if progress_for(&message, token) {
                return Streamed::Partial(progress_value(message));
            }
        }
    }

    /// Issue a request and await its response.
    pub async fn request(&mut self, method: &str, params: Value) -> Message {
        let id = self.send(method, params).await;
        self.response(&id).await
    }

    /// Pull one source without a client-provided version or previous identity.
    pub async fn pull_document(&mut self, uri: &str) -> Message {
        self.request(
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":uri}}),
        )
        .await
    }

    /// Pull an explicit synchronized version with an optional previous identity.
    pub async fn pull_version(
        &mut self,
        uri: &str,
        version: i32,
        previous: Option<&str>,
    ) -> Message {
        self.request(
            "textDocument/diagnostic",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "previousResultId": previous
            }),
        )
        .await
    }

    /// Request the current source using a real previous report identity.
    pub async fn pull_previous(&mut self, uri: &str, previous: &str) -> Message {
        self.request(
            "textDocument/diagnostic",
            json!({
                "textDocument": { "uri": uri }, "previousResultId": previous
            }),
        )
        .await
    }

    /// Supply actual full-sync editor input to the running protocol consumer.
    pub async fn open_document(&mut self, uri: &str, version: i32, text: &str) {
        self.notify("textDocument/didOpen", json!({
            "textDocument": { "uri": uri, "languageId": "json", "version": version, "text": text }
        })).await;
    }

    /// Replace full-sync input through the same public protocol path as an editor.
    pub async fn change_document(&mut self, uri: &str, version: i32, text: &str) {
        self.notify("textDocument/didChange", json!({
            "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }]
        })).await;
    }

    /// Preserve the full shutdown contract and assert the actual child exit code.
    pub async fn assert_clean_exit(&mut self) {
        let status = self.shutdown_and_exit().await;
        assert!(status.success(), "clean exit, got {status:?}");
    }

    /// Await the next notification with `method`, skipping other traffic.
    pub async fn notification(&mut self, method: &str) -> Message {
        loop {
            let message = self.next_message().await;
            if message.method.as_deref() == Some(method) {
                return message;
            }
        }
    }

    /// Supporting metadata: backend work count and snapshot revision.
    pub async fn work(&mut self) -> Value {
        result_of(&self.request("reference/work", json!({})).await)
    }

    /// Read the next framed message or fail after the responsiveness guard.
    pub async fn next_message(&mut self) -> Message {
        timeout(READ_GUARD, read_message(&mut self.reader))
            .await
            .expect("server stays responsive")
            .expect("read framed message")
    }

    /// Shutdown, exit, and return the child's exit status.
    pub async fn shutdown_and_exit(&mut self) -> ExitStatus {
        let shutdown = self.request("shutdown", Value::Null).await;
        assert!(
            shutdown.error.is_none(),
            "shutdown must succeed: {:?}",
            shutdown.error
        );
        // A wire `result: null` deserializes to `None` in `Option<Value>`.
        assert_eq!(
            shutdown.result, None,
            "shutdown answers null: {:?}",
            shutdown.error
        );
        self.notify("exit", Value::Null).await;
        self.child.wait().await.expect("child exits")
    }

    /// Close stdin and return the child's exit status.
    pub async fn close_stdin_and_wait(&mut self) -> ExitStatus {
        self.writer = None;
        self.child.wait().await.expect("child exits")
    }

    /// Wait for the child to exit on its own (after `exit` or EOF).
    pub async fn wait_for_exit(&mut self) -> ExitStatus {
        self.child.wait().await.expect("child exits")
    }

    /// Assert stdout reaches EOF after the child exits, draining any
    /// trailing frames the child flushed before exiting.
    pub async fn read_eof(&mut self) {
        loop {
            let outcome = timeout(READ_GUARD, read_message(&mut self.reader))
                .await
                .expect("eof within guard");
            match outcome {
                Err(FramingError::Closed) => return,
                Ok(_) => {}
                error => {
                    let _ = error.expect("expected a valid frame or clean EOF");
                }
            }
        }
    }
}

/// Whether `message` is a `$/progress` partial for `token`.
fn progress_for(message: &Message, token: &Value) -> bool {
    message.method.as_deref() == Some("$/progress")
        && message
            .params
            .as_ref()
            .is_some_and(|params| params.get("token") == Some(token))
}

/// The streamed partial payload of a `$/progress` notification.
fn progress_value(message: Message) -> Value {
    message
        .params
        .expect("progress params")
        .get("value")
        .cloned()
        .expect("progress value")
}

/// Resolve the exact prebuilt producer; never select a stale artifact by time.
pub fn reference_binary() -> PathBuf {
    reference_program::reference_binary().expect("real reference CLI producer was built")
}

/// The `result` of a successful response; wire `null` reads as `None` in
/// `Option<Value>`, so both forms normalize to JSON null here.
pub fn result_of(message: &Message) -> Value {
    message.result.clone().unwrap_or(Value::Null)
}

/// The `error` of a failed response.
pub fn error_of(message: &Message) -> JsonRpcError {
    message.error.clone().expect("response carries an error")
}

/// Native read failures remain informative errors rather than successful absence.
pub fn assert_backend_failure(message: &Message) {
    assert!(
        message.result.is_none(),
        "a failed read cannot report success"
    );
    let error = error_of(message);
    assert_eq!(error.code, -32603);
    assert!(
        error.message.contains("read failed"),
        "honest backend failure: {error:?}"
    );
}

/// Parse the `p:…|d:…|g:…` snapshot identity into its fields.
pub fn identity(result_id: &str) -> (u64, u64, u64) {
    let parts: Vec<&str> = result_id.split('|').collect();
    let field = |part: &str| {
        part.split(':')
            .nth(1)
            .expect("identity field follows its colon")
            .parse::<u64>()
            .expect("snapshot identity field is numeric")
    };
    (field(parts[0]), field(parts[1]), field(parts[2]))
}

/// The file URI for a path inside a fixture workspace.
pub fn uri_for(path: impl AsRef<Path>) -> String {
    path_to_uri(path.as_ref()).expect("fixture uri")
}

/// A workspace fixture with `count` clean JSON files at the watched root.
pub fn bulk_workspace(count: usize) -> TempDir {
    let workspace = TempDir::new().expect("tempdir");
    for index in 0..count {
        let name = format!("scan-{index:04}.json");
        std::fs::write(workspace.path().join(name), format!("{{\"n\": {index}}}"))
            .expect("write scan file");
    }
    workspace
}

/// Initialize a session rooted at `root`.
pub async fn initialize_at(client: &mut ReferenceClient, root: &Path) {
    let root_uri = path_to_uri(root).expect("root uri");
    let initialized = client
        .request(
            "initialize",
            json!({ "processId": Value::Null, "rootUri": root_uri, "capabilities": {} }),
        )
        .await;
    let capabilities = result_of(&initialized);
    assert_eq!(
        capabilities["capabilities"]["diagnosticProvider"]["workspaceDiagnostics"], true,
        "pull diagnostics advertised for the watched root"
    );
    client.notify("notifications/initialized", json!({})).await;
}

/// Plain source contracts reuse the same real workspace and protocol initialization.
pub async fn workspace_session(count: usize) -> (TempDir, ReferenceClient) {
    let workspace = bulk_workspace(count);
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, workspace.path()).await;
    (workspace, client)
}
