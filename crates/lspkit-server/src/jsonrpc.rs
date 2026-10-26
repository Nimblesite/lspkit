//! Content-Length-framed JSON-RPC 2.0 over async I/O.
//!
//! This is the primitive every LSP server uses to talk to its client. We
//! own the framing and routing so the toolkit does not depend on an
//! unmaintained third-party LSP server crate.

use std::io;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

/// JSON-RPC 2.0 message identifier. Either a request ID or absent for notifications.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    /// Numeric ID.
    Number(i64),
    /// String ID.
    String(String),
}

/// An incoming JSON-RPC message.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Optional ID — present for requests and responses, absent for notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
    /// Method name — present for requests and notifications, absent for responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Parameters — present for requests and notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Result — present for successful responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error — present for failed responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Numeric error code (see JSON-RPC spec for reserved ranges).
    pub code: i32,
    /// Short human-readable message.
    pub message: String,
    /// Optional structured details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    /// Construct a new error with `code` and `message` and no `data`.
    #[must_use]
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Attach structured `data` to the error.
    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

impl Message {
    /// Construct a request with `id`, `method`, and `params`.
    #[must_use]
    pub fn request(id: RequestId, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id: Some(id),
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    /// Construct a notification with `method` and `params`.
    #[must_use]
    pub fn notification(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id: None,
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    /// Construct a successful response carrying `result`.
    #[must_use]
    pub fn response(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id: Some(id),
            method: None,
            params: None,
            result: Some(result),
            error: None,
        }
    }

    /// Construct a failed response carrying `error`.
    #[must_use]
    pub fn response_error(id: RequestId, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id: Some(id),
            method: None,
            params: None,
            result: None,
            error: Some(error),
        }
    }
}

/// Errors from reading or writing JSON-RPC messages.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum FramingError {
    /// Underlying I/O failure.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Header could not be parsed; numeric failures retain their source.
    #[error("malformed Content-Length header: {0}")]
    BadHeader(String, #[source] Option<std::num::ParseIntError>),
    /// Message body was not valid JSON.
    #[error("malformed JSON body: {0}")]
    BadJson(#[from] serde_json::Error),
    /// Peer closed the stream.
    #[error("peer closed the stream")]
    Closed,
}

/// Read one Content-Length-framed JSON-RPC message from `reader`.
///
/// # Errors
/// Returns [`FramingError`] on I/O failure, header parse failure, or invalid JSON.
pub async fn read_message<R>(reader: &mut BufReader<R>) -> Result<Message, FramingError>
where
    R: AsyncReadExt + Unpin,
{
    let mut content_length: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            return Err(FramingError::Closed);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse()
                    .map_err(|source| FramingError::BadHeader(trimmed.to_owned(), Some(source)))?,
            );
        }
    }
    let length =
        content_length.ok_or_else(|| FramingError::BadHeader("missing".to_owned(), None))?;
    let mut body = vec![0u8; length];
    let _ = reader.read_exact(&mut body).await?;
    let message: Message = serde_json::from_slice(&body)?;
    Ok(message)
}

/// Writer half of a JSON-RPC connection, serialized so multiple tasks can send.
#[derive(Debug)]
pub struct MessageWriter<W: AsyncWrite + Unpin + Send> {
    /// Serializes complete frames so concurrent writers cannot interleave bytes.
    inner: Mutex<W>,
}

impl<W: AsyncWrite + Unpin + Send> MessageWriter<W> {
    /// Wrap an async writer.
    pub fn new(writer: W) -> Self {
        Self {
            inner: Mutex::new(writer),
        }
    }

    /// Write a single Content-Length-framed message.
    ///
    /// # Errors
    /// Returns [`FramingError::Io`] on writer failure or
    /// [`FramingError::BadJson`] if the message cannot be serialized.
    pub async fn write_message(&self, message: &Message) -> Result<(), FramingError> {
        let body = serde_json::to_vec(message)?;
        let mut guard = self.inner.lock().await;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        guard.write_all(header.as_bytes()).await?;
        guard.write_all(&body).await?;
        guard.flush().await?;
        Ok(())
    }
}
