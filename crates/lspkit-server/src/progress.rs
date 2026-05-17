//! Work-done progress sink.
//!
//! Wraps the LSP `window/workDoneProgress` protocol. The sink is a no-op when
//! the client did not advertise the capability — long-running operations can
//! report unconditionally without checking.

use std::sync::Arc;

use async_trait::async_trait;

/// One progress step.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    /// Token identifying the progress stream.
    pub token: String,
    /// Human-readable message.
    pub message: String,
    /// Optional `[0, 100]` percentage.
    pub percentage: Option<u32>,
    /// `true` when this is the final update for the stream.
    pub done: bool,
}

/// A receiver for progress updates.
#[async_trait]
pub trait ProgressReporter: Send + Sync + 'static {
    /// Receive an update. Implementations MUST NOT block.
    async fn report(&self, update: ProgressUpdate);
}

/// No-op reporter used when the client did not request progress.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopReporter;

#[async_trait]
impl ProgressReporter for NoopReporter {
    async fn report(&self, _update: ProgressUpdate) {}
}

/// A handle that publishes updates to its inner reporter.
#[derive(Clone)]
pub struct ProgressSink {
    inner: Arc<dyn ProgressReporter>,
    token: String,
}

impl ProgressSink {
    /// Construct a sink that forwards updates to `inner` under `token`.
    pub fn new(inner: Arc<dyn ProgressReporter>, token: impl Into<String>) -> Self {
        Self { inner, token: token.into() }
    }

    /// No-op sink that drops every update.
    #[must_use]
    pub fn noop() -> Self {
        Self { inner: Arc::new(NoopReporter), token: String::new() }
    }

    /// Send a single update.
    pub async fn report(&self, message: impl Into<String>, percentage: Option<u32>) {
        self.inner
            .report(ProgressUpdate {
                token: self.token.clone(),
                message: message.into(),
                percentage,
                done: false,
            })
            .await;
    }

    /// Send a final update and close the stream.
    pub async fn finish(&self, message: impl Into<String>) {
        self.inner
            .report(ProgressUpdate {
                token: self.token.clone(),
                message: message.into(),
                percentage: Some(100),
                done: true,
            })
            .await;
    }
}

impl std::fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressSink").field("token", &self.token).finish_non_exhaustive()
    }
}
