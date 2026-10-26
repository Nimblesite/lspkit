//! The sampler contract every CPU backend implements. Implements [PROF-SAMPLER].
//!
//! A backend produces [`SampleBatch`]es — one per sampling tick — on any
//! thread or task it likes and hands the session manager a [`SamplerHandle`].
//! Live backends (an in-process stack sampler, an elevated helper, a tailed
//! cooperative file) stream batches while recording; file backends (an
//! external tool that writes a profile on exit) deliver everything when
//! stopped. The handle's completion signal lets `stop` wait for the second
//! kind without blocking the first.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::error::ProfilerError;
use crate::wire::{AttachErrorKind, ThreadSample};

/// Capacity of the batch channel between a backend and its session.
pub const SAMPLE_CHANNEL_DEPTH: usize = 256;

/// The stacks of every sampled thread at one sampling tick.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleBatch {
    /// One stack per sampled thread, frames leaf first.
    pub samples: Vec<ThreadSample>,
    /// Seconds this tick represents; `None` means `1 / sample_rate`.
    pub weight_secs: Option<f64>,
}

impl SampleBatch {
    /// A batch weighted at the session's sampling interval.
    #[must_use]
    pub fn new(samples: Vec<ThreadSample>) -> Self {
        Self {
            samples,
            weight_secs: None,
        }
    }
}

/// Why a backend could not start sampling. Implements [PROF-ERRORS].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SamplerError {
    /// The target PID was not found.
    #[error("Process not found: PID {0}")]
    ProcessNotFound(u32),
    /// The target does not run the backend's runtime.
    #[error("PID {pid} is not a {runtime} process")]
    UnsupportedRuntime {
        /// The target PID.
        pid: u32,
        /// Display name of the runtime that rejected it.
        runtime: String,
    },
    /// The OS refused access; the message carries the remedy.
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    /// A required external tool is not installed; the message says how to install it.
    #[error("{0}")]
    ToolMissing(String),
    /// Any other attach failure.
    #[error("Failed to attach to process: {0}")]
    AttachFailed(String),
}

impl SamplerError {
    /// Map a classified attach failure onto the matching variant, so every
    /// attach path (in-process, helper, external tool) reports identical,
    /// distinct failure modes.
    #[must_use]
    pub fn from_kind(kind: &AttachErrorKind, pid: u32, runtime: &str, message: String) -> Self {
        match kind {
            AttachErrorKind::ProcessNotFound => Self::ProcessNotFound(pid),
            AttachErrorKind::UnsupportedRuntime => Self::UnsupportedRuntime {
                pid,
                runtime: runtime.to_owned(),
            },
            AttachErrorKind::PermissionDenied => Self::PermissionDenied(message),
            AttachErrorKind::AttachFailed => Self::AttachFailed(message),
        }
    }
}

/// Producer side of a sampler: what a backend's sampling loop owns.
#[derive(Debug, Clone)]
pub struct SamplerSink {
    /// Batches flow to the session through this sender.
    pub sender: mpsc::Sender<SampleBatch>,
    /// Set when the session wants sampling to end.
    pub stop_flag: Arc<AtomicBool>,
}

impl SamplerSink {
    /// Whether the session asked the producer to stop.
    #[must_use]
    pub fn stop_requested(&self) -> bool {
        self.stop_flag.load(Ordering::Relaxed)
    }
}

/// A cooperative stop control that can interrupt a pending snapshot barrier.
#[derive(Debug, Clone)]
pub struct SamplerStop {
    /// Shared stop signal observed by the backend without holding session locks.
    flag: Arc<AtomicBool>,
}

impl SamplerStop {
    /// Ask the producer to finish without waiting for its recording lock.
    pub fn request(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }
}

/// Consumer side of a sampler: what the session manager owns.
#[derive(Debug)]
pub struct SamplerHandle {
    /// Batches produced so far.
    pub receiver: mpsc::Receiver<SampleBatch>,
    /// Runtime version the backend detected in the target.
    pub runtime_version: String,
    /// The profiled process.
    pub pid: u32,
    /// Signals the producer to stop.
    stop_flag: Arc<AtomicBool>,
    /// Resolves once the producer has delivered its last batch.
    completion: Option<oneshot::Receiver<()>>,
    /// How long `stop` waits for the producer to finish.
    finish_timeout: Duration,
    /// Optional barrier for backends that buffer unfinished recording segments.
    snapshots: Option<mpsc::UnboundedSender<oneshot::Sender<()>>>,
}

/// Everything a backend needs to start producing samples.
#[derive(Debug)]
pub struct SamplerParts {
    /// Hand this to the producer.
    pub sink: SamplerSink,
    /// Fire this (or drop it) when the producer has delivered its last batch.
    pub done: oneshot::Sender<()>,
    /// Return this to the session manager.
    pub handle: SamplerHandle,
}

impl SamplerHandle {
    /// Create a connected producer/consumer pair for one session.
    ///
    /// `finish_timeout` bounds how long `stop` waits for the producer — a few
    /// seconds for a live sampler, longer for a tool that converts its
    /// recording on exit.
    #[must_use]
    pub fn channel(pid: u32, runtime_version: String, finish_timeout: Duration) -> SamplerParts {
        let (sender, receiver) = mpsc::channel(SAMPLE_CHANNEL_DEPTH);
        let (done, completion) = oneshot::channel();
        let stop_flag = Arc::new(AtomicBool::new(false));
        SamplerParts {
            sink: SamplerSink {
                sender,
                stop_flag: Arc::clone(&stop_flag),
            },
            done,
            handle: Self::from_channel(
                receiver,
                (pid, runtime_version),
                stop_flag,
                completion,
                finish_timeout,
            ),
        }
    }

    /// Assemble the consumer from the same channel endpoints as its producer.
    fn from_channel(
        receiver: mpsc::Receiver<SampleBatch>,
        identity: (u32, String),
        stop_flag: Arc<AtomicBool>,
        completion: oneshot::Receiver<()>,
        finish_timeout: Duration,
    ) -> Self {
        let (pid, runtime_version) = identity;
        Self {
            receiver,
            runtime_version,
            pid,
            stop_flag,
            completion: Some(completion),
            finish_timeout,
            snapshots: None,
        }
    }

    /// Signal the producer to stop without waiting.
    pub fn signal_stop(&self) {
        self.stop_control().request();
    }

    /// A stop control that remains available while a snapshot owns the sampler.
    #[must_use]
    pub fn stop_control(&self) -> SamplerStop {
        SamplerStop {
            flag: Arc::clone(&self.stop_flag),
        }
    }

    /// Enable backend snapshot barriers. Acknowledge only after delivering the
    /// buffered samples, so the session can drain them before reporting.
    pub fn snapshot_requests(&mut self) -> mpsc::UnboundedReceiver<oneshot::Sender<()>> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.snapshots = Some(sender);
        receiver
    }

    /// Flush a buffered backend while draining its samples. Implements [PROF-CPU-SNAPSHOT].
    ///
    /// # Errors
    /// Returns an error if the producer exits or cannot finish its barrier in time.
    pub async fn snapshot(
        &mut self,
        on_batch: impl FnMut(SampleBatch),
    ) -> Result<(), ProfilerError> {
        let Some(requests) = self.snapshots.as_ref() else {
            return Ok(());
        };
        let (done, completion) = oneshot::channel();
        requests.send(done).map_err(|error| {
            ProfilerError::Other(format!("Sampler snapshot channel closed: {error}"))
        })?;
        self.drain_until(completion, on_batch).await
    }

    /// Signal the producer to stop and wait (bounded) until it has delivered
    /// its last batch, handing every batch that arrives meanwhile to
    /// `on_batch`. A file backend delivers its whole recording at stop, so
    /// waiting without draining would fill the channel and deadlock.
    pub async fn stop(&mut self, on_batch: impl FnMut(SampleBatch)) {
        self.signal_stop();
        let Some(completion) = self.completion.take() else {
            return;
        };
        match self.drain_until(completion, on_batch).await {
            Ok(()) => debug!(pid = self.pid, "sampler finished"),
            Err(error) => warn!(pid = self.pid, %error, "sampler did not finish cleanly"),
        }
    }

    /// Drain batches until the producer acknowledges completion or times out.
    async fn drain_until(
        &mut self,
        mut completion: oneshot::Receiver<()>,
        mut on_batch: impl FnMut(SampleBatch),
    ) -> Result<(), ProfilerError> {
        let deadline = tokio::time::sleep(self.finish_timeout);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                result = &mut completion => return result.map_err(|error| ProfilerError::Other(format!("Sampler ended before completing its barrier: {error}"))),
                () = &mut deadline => return Err(ProfilerError::Other("Sampler barrier timed out".to_owned())),
                Some(batch) = self.receiver.recv() => on_batch(batch),
            }
        }
    }
}

impl Drop for SamplerHandle {
    fn drop(&mut self) {
        // Never block in Drop — the producer sees the flag and exits.
        self.signal_stop();
    }
}
