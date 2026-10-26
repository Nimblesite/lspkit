//! Order partial and terminal replies for one request. [LSP-PULL-CANCELLATION]

use std::sync::{Arc, Mutex};

/// A per-request output lifetime; it never stores response values.
#[derive(Clone, Default)]
pub struct ReplyGate(Arc<Mutex<bool>>);

/// Why a partial or terminal reply could not be emitted.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EmissionError<E: std::fmt::Debug + std::fmt::Display> {
    /// The terminal reply has already closed this stream.
    #[error("request reply stream is closed")]
    Closed,
    /// The synchronization state is unavailable.
    #[error("request reply stream lock unavailable")]
    Unavailable,
    /// The caller's emission failed.
    #[error("request reply emission failed: {0}")]
    Write(E),
}

impl ReplyGate {
    /// Emit a partial reply only while the request is open.
    ///
    /// # Errors
    /// Returns a closed-stream, poisoned-lock or caller emission error.
    pub fn emit<E: std::fmt::Debug + std::fmt::Display>(
        &self,
        emit: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), EmissionError<E>> {
        self.with_open(false, emit)
    }

    /// Close the stream and emit its terminal reply under the same lock.
    ///
    /// # Errors
    /// Returns a closed-stream, poisoned-lock or caller emission error.
    pub fn finish<E: std::fmt::Debug + std::fmt::Display>(
        &self,
        emit: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), EmissionError<E>> {
        self.with_open(true, emit)
    }

    fn with_open<E: std::fmt::Debug + std::fmt::Display>(
        &self,
        finish: bool,
        emit: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), EmissionError<E>> {
        let mut closed = self.0.lock().map_err(|_error| EmissionError::Unavailable)?;
        if *closed {
            return Err(EmissionError::Closed);
        }
        *closed = finish;
        emit().map_err(EmissionError::Write)
    }
}
