//! The framed stdio client driving the real reference server binary.
//!
//! Every method that touches the child or its pipes returns [`StepResult`];
//! the `#[test]` callers `expect` on them so a failure names the step.

use std::process::ExitStatus;

use lspkit_server::jsonrpc::read_message;
use lspkit_server::{FramingError, Message, MessageWriter, RequestId};
use serde_json::{json, Value};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::time::timeout;

use super::{result_of, StepError, StepResult, READ_GUARD};

/// The next meaningful traffic for an in-flight streamed request.
pub enum Streamed {
    /// One `$/progress` partial carrying the request's token.
    Partial(Value),
    /// The terminal response for the request id.
    Terminal(Message),
}

impl Streamed {
    /// A real partial at the in-flight scan barrier; a terminal response
    /// proves the scan already ended, which fails the step.
    pub fn partial(self) -> StepResult<Value> {
        match self {
            Self::Partial(value) => Ok(value),
            Self::Terminal(message) => {
                Err(format!("scan is in flight, got terminal: {message:?}").into())
            }
        }
    }
}

/// One framed stdio client of the reference server.
pub struct ReferenceClient {
    /// The real child process behind both pipes.
    child: Child,
    /// The open stdin pipe; dropping it signals EOF to the child.
    writer: Option<ChildStdin>,
    /// Buffered stdout frames read back from the child.
    reader: BufReader<ChildStdout>,
    /// Monotonic source of fresh request identities.
    next_id: i64,
}

impl ReferenceClient {
    /// Spawn the real example binary with piped stdio.
    pub fn spawn() -> StepResult<Self> {
        let mut child = super::reference_program::spawn_reference(&[])?;
        let stdin = child.stdin.take().ok_or("reference stdin pipe")?;
        let stdout = child.stdout.take().ok_or("reference stdout pipe")?;
        Ok(Self {
            child,
            writer: Some(stdin),
            reader: BufReader::new(stdout),
            next_id: 1,
        })
    }

    /// Send a request and return its id.
    pub async fn send(&mut self, method: &str, params: Value) -> StepResult<RequestId> {
        let id = self.take_id();
        self.write(Message::request(id.clone(), method, params))
            .await?;
        Ok(id)
    }

    /// Allocate the next request identity once for ordinary or batched traffic.
    fn take_id(&mut self) -> RequestId {
        let id = RequestId::Number(self.next_id);
        self.next_id += 1;
        id
    }

    /// Put the request and cancellation in one native write, before either is read.
    pub async fn send_cancelled(&mut self, method: &str, params: Value) -> StepResult<RequestId> {
        let id = self.take_id();
        let cancellation = Message::notification("$/cancelRequest", json!({ "id": id.clone() }));
        self.write_batch(&[Message::request(id.clone(), method, params), cancellation])
            .await?;
        Ok(id)
    }

    /// Send a request reusing an explicit id; ids stay usable after a reply.
    pub async fn send_reusing(
        &mut self,
        id: &RequestId,
        method: &str,
        params: Value,
    ) -> StepResult<()> {
        if let RequestId::Number(number) = id {
            self.next_id = self.next_id.max(*number + 1);
        }
        self.write(Message::request(id.clone(), method, params))
            .await
    }

    /// Send a notification (no response expected).
    pub async fn notify(&mut self, method: &str, params: Value) -> StepResult<()> {
        self.write(Message::notification(method, params)).await
    }

    /// Write one frame through the open stdin pipe.
    async fn write(&mut self, message: Message) -> StepResult<()> {
        let stdin = self.writer.as_mut().ok_or("stdin open")?;
        MessageWriter::new(stdin).write_message(&message).await?;
        Ok(())
    }

    /// Encode with the shared writer and send the complete native batch once.
    pub async fn write_batch(&mut self, messages: &[Message]) -> StepResult<()> {
        let mut bytes = Vec::new();
        {
            let writer = MessageWriter::new(&mut bytes);
            for message in messages {
                writer.write_message(message).await?;
            }
        }
        let input = self.writer.as_mut().ok_or("stdin open")?;
        input.write_all(&bytes).await?;
        input.flush().await?;
        Ok(())
    }

    /// Read the response for `id`, skipping unrelated traffic.
    pub async fn response(&mut self, id: &RequestId) -> StepResult<Message> {
        loop {
            let message = self.next_message().await?;
            if message.id.as_ref() == Some(id) {
                return Ok(message);
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
    ) -> StepResult<(Message, Vec<Value>)> {
        let id = self.send(method, params).await?;
        self.collect_stream(&id, token).await
    }

    /// Drain an already-started real request without losing its partial reports.
    pub async fn collect_stream(
        &mut self,
        id: &RequestId,
        token: &Value,
    ) -> StepResult<(Message, Vec<Value>)> {
        let mut partials = Vec::new();
        loop {
            match self.next_streamed(id, token).await? {
                Streamed::Partial(value) => partials.push(value),
                Streamed::Terminal(message) => return Ok((message, partials)),
            }
        }
    }

    /// Start the same standard workspace scan through the actual public method.
    pub async fn start_scan(&mut self, token: &Value) -> StepResult<RequestId> {
        self.send(
            "workspace/diagnostic",
            json!({
                "previousResultIds": [], "partialResultToken": token
            }),
        )
        .await
    }

    /// A partial proves that the real scan is active; callers assert its shape.
    pub async fn first_partial(&mut self, id: &RequestId, token: &Value) -> StepResult<Value> {
        self.next_streamed(id, token).await?.partial()
    }

    /// Next meaningful traffic for one streamed request: the next `$/progress`
    /// partial carrying `token`, or the terminal response for `id`.
    pub async fn next_streamed(&mut self, id: &RequestId, token: &Value) -> StepResult<Streamed> {
        loop {
            let message = self.next_message().await?;
            if message.id.as_ref() == Some(id) {
                return Ok(Streamed::Terminal(message));
            }
            if progress_for(&message, token) {
                return Ok(Streamed::Partial(progress_value(&message)?));
            }
        }
    }

    /// Issue a request and await its response.
    pub async fn request(&mut self, method: &str, params: Value) -> StepResult<Message> {
        let id = self.send(method, params).await?;
        self.response(&id).await
    }

    /// Pull one source without a client-provided version or previous identity.
    pub async fn pull_document(&mut self, uri: &str) -> StepResult<Message> {
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
    ) -> StepResult<Message> {
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
    pub async fn pull_previous(&mut self, uri: &str, previous: &str) -> StepResult<Message> {
        self.request(
            "textDocument/diagnostic",
            json!({
                "textDocument": { "uri": uri }, "previousResultId": previous
            }),
        )
        .await
    }

    /// Supply actual full-sync editor input to the running protocol consumer.
    pub async fn open_document(&mut self, uri: &str, version: i32, text: &str) -> StepResult<()> {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": { "uri": uri, "languageId": "json", "version": version, "text": text }
            }),
        )
        .await
    }

    /// Replace full-sync input through the same public protocol path as an editor.
    pub async fn change_document(&mut self, uri: &str, version: i32, text: &str) -> StepResult<()> {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }]
            }),
        )
        .await
    }

    /// Await the next notification with `method`, skipping other traffic.
    pub async fn notification(&mut self, method: &str) -> StepResult<Message> {
        loop {
            let message = self.next_message().await?;
            if message.method.as_deref() == Some(method) {
                return Ok(message);
            }
        }
    }

    /// Supporting metadata: backend work count and snapshot revision.
    pub async fn work(&mut self) -> StepResult<Value> {
        let reply = self.request("reference/work", json!({})).await?;
        Ok(result_of(&reply))
    }

    /// Read the next framed message or fail after the responsiveness guard.
    pub async fn next_message(&mut self) -> StepResult<Message> {
        let framed = timeout(READ_GUARD, read_message(&mut self.reader))
            .await
            .map_err(|elapsed| -> StepError {
                format!("server stays responsive: {elapsed}").into()
            })?;
        framed.map_err(|error| -> StepError { format!("read framed message: {error}").into() })
    }

    /// Shutdown, exit, and return the child's exit status; a shutdown reply
    /// carrying an error or a non-null result fails the step.
    pub async fn shutdown_and_exit(&mut self) -> StepResult<ExitStatus> {
        let shutdown = self.request("shutdown", Value::Null).await?;
        if let Some(error) = shutdown.error.clone() {
            return Err(format!("shutdown must succeed: {error:?}").into());
        }
        // A wire `result: null` deserializes to `None` in `Option<Value>`.
        if shutdown.result.is_some() {
            return Err("shutdown answers null".into());
        }
        self.notify("exit", Value::Null).await?;
        Ok(self.child.wait().await?)
    }

    /// Close stdin and return the child's exit status.
    pub async fn close_stdin_and_wait(&mut self) -> StepResult<ExitStatus> {
        self.writer = None;
        Ok(self.child.wait().await?)
    }

    /// Wait for the child to exit on its own (after `exit` or EOF).
    pub async fn wait_for_exit(&mut self) -> StepResult<ExitStatus> {
        Ok(self.child.wait().await?)
    }

    /// Assert stdout reaches EOF after the child exits, draining any
    /// trailing frames the child flushed before exiting.
    pub async fn read_eof(&mut self) -> StepResult<()> {
        loop {
            let outcome = timeout(READ_GUARD, read_message(&mut self.reader))
                .await
                .map_err(|elapsed| -> StepError {
                    format!("eof within guard: {elapsed}").into()
                })?;
            match outcome {
                Ok(_) => {}
                Err(FramingError::Closed) => return Ok(()),
                Err(error) => {
                    return Err(format!("expected a valid frame or clean EOF: {error}").into());
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
fn progress_value(message: &Message) -> StepResult<Value> {
    message
        .params
        .as_ref()
        .and_then(|params| params.get("value"))
        .cloned()
        .ok_or_else(|| format!("progress carries its value: {message:?}").into())
}
