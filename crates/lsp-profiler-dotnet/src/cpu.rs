//! `EventPipe` CPU sampling through the engine. Implements [PROF-DOTNET-CPU].
//!
//! The engine records in segments and streams each segment's ticks once it is
//! symbolized, until it reads a line on stdin. Ticks therefore arrive a segment
//! behind, not live: the backend reports no live progress, a snapshot sees
//! every finished segment, and a target that exits keeps them.

use std::time::Duration;

use async_trait::async_trait;
use lsp_profiler::runtime::{CpuBackend, SamplerConfig};
use lsp_profiler::sampler::{SampleBatch, SamplerError, SamplerHandle, SamplerSink};
use lsp_profiler::wire::{AttachErrorKind, CpuCapabilities, StackFrame, ThreadSample};
use tokio::sync::oneshot;
use tracing::{debug, error, info};

use crate::engine::{Engine, EngineError, RunningEngine, StopSignal};
use crate::protocol::{EngineMessage, ErrorKind, FrameRecord, TickRecord};
use crate::RUNTIME_DISPLAY_NAME;

/// Symbolizing a long trace takes a while; `stop` waits this long for the last tick.
const FINISH_TIMEOUT: Duration = Duration::from_secs(120);
/// How often the forwarder checks whether the session asked to stop.
const STOP_POLL: Duration = Duration::from_millis(100);

/// The .NET CPU backend.
#[derive(Debug)]
pub struct DotNetCpu {
    engine: Engine,
}

impl DotNetCpu {
    /// A backend driving `engine`.
    #[must_use]
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl CpuBackend for DotNetCpu {
    fn capabilities(&self) -> CpuCapabilities {
        CpuCapabilities {
            live_progress: false,
            line_level: true,
            native_frames: true,
            cooperative: false,
        }
    }

    async fn attach(&self, config: &SamplerConfig) -> Result<SamplerHandle, SamplerError> {
        let mut args = vec!["cpu".to_owned(), "--pid".to_owned(), config.pid.to_string()];
        if config.include_native {
            args.push("--native".to_owned());
        }
        let mut running = self
            .engine
            .spawn(&args)
            .map_err(|error| sampler_error(error, config.pid))?;
        let version = await_attached(&mut running, config.pid).await?;
        info!(pid = config.pid, "attached to .NET process");
        let parts = SamplerHandle::channel(config.pid, version, FINISH_TIMEOUT);
        drop(tokio::spawn(forward(
            running,
            parts.sink,
            config.duration,
            parts.done,
        )));
        Ok(parts.handle)
    }
}

async fn await_attached(running: &mut RunningEngine, pid: u32) -> Result<String, SamplerError> {
    match running
        .next()
        .await
        .map_err(|error| sampler_error(error, pid))?
    {
        Some(EngineMessage::Attached {
            runtime_version, ..
        }) => Ok(runtime_version.unwrap_or_default()),
        Some(EngineMessage::Failure {
            error_kind,
            message,
        }) => Err(sampler_error(
            EngineError::Failed {
                kind: error_kind,
                message,
            },
            pid,
        )),
        Some(other) => Err(SamplerError::AttachFailed(format!(
            "unexpected engine message before attach: {other:?}"
        ))),
        None => Err(SamplerError::AttachFailed(
            "the .NET profiling engine exited before attaching".to_owned(),
        )),
    }
}

/// Forward every tick the engine streams while it records; stop it when the
/// session asks or the duration ends.
async fn forward(
    mut running: RunningEngine,
    sink: SamplerSink,
    limit: Option<Duration>,
    done: oneshot::Sender<()>,
) {
    let stopper = tokio::spawn(stop_when_asked(running.stop_signal(), sink.clone(), limit));
    relay(&mut running, &sink).await;
    stopper.abort();
    running.finish().await;
    let _ = done.send(());
}

/// Send the engine its stop line once the session asks or the duration ends.
async fn stop_when_asked(signal: StopSignal, sink: SamplerSink, limit: Option<Duration>) {
    wait_for_stop(&sink, limit).await;
    signal.send().await;
}

/// Hand every tick to the session until the engine has delivered the last one.
async fn relay(running: &mut RunningEngine, sink: &SamplerSink) {
    let mut frames = FrameTable::default();
    loop {
        match running.next().await {
            Ok(Some(EngineMessage::Frames { items })) => frames.extend(items),
            Ok(Some(EngineMessage::Ticks { items })) => {
                if !deliver(sink, &frames, items).await {
                    break;
                }
            }
            Ok(Some(EngineMessage::Failure { message, .. })) => {
                error!(message, "the .NET profiling engine failed while sampling");
                break;
            }
            Ok(Some(EngineMessage::Stopped { tick_count })) => {
                debug!(tick_count, "the .NET profiling engine delivered every tick");
                break;
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(error) => {
                error!(%error, "reading the .NET profiling engine failed");
                break;
            }
        }
    }
}

async fn wait_for_stop(sink: &SamplerSink, limit: Option<Duration>) {
    let started = tokio::time::Instant::now();
    while !sink.stop_requested() && limit.is_none_or(|limit| started.elapsed() < limit) {
        tokio::time::sleep(STOP_POLL).await;
    }
}

async fn deliver(sink: &SamplerSink, frames: &FrameTable, ticks: Vec<TickRecord>) -> bool {
    for tick in ticks {
        if sink.sender.send(frames.batch(tick)).await.is_err() {
            return false;
        }
    }
    true
}

/// The engine's frame table: IDs are positions in arrival order.
#[derive(Debug, Default)]
struct FrameTable {
    frames: Vec<StackFrame>,
}

impl FrameTable {
    fn extend(&mut self, items: Vec<FrameRecord>) {
        self.frames.extend(items.into_iter().map(|f| StackFrame {
            name: f.name,
            file: f.file,
            line: f.line,
            module: f.module,
        }));
    }

    fn batch(&self, tick: TickRecord) -> SampleBatch {
        let samples = tick
            .threads
            .into_iter()
            .map(|thread| ThreadSample {
                thread_id: thread.tid,
                thread_name: None,
                active: thread.active,
                frames: thread
                    .stack
                    .iter()
                    .filter_map(|id| self.frames.get(*id).cloned())
                    .collect(),
            })
            .collect();
        SampleBatch {
            samples,
            weight_secs: Some(tick.weight_ms / 1000.0),
        }
    }
}

/// Map an engine failure onto the sampler's classified errors.
fn sampler_error(error: EngineError, pid: u32) -> SamplerError {
    match error {
        EngineError::Missing(message) => SamplerError::ToolMissing(message),
        EngineError::Failed { kind, message } => match attach_kind(kind) {
            Some(kind) => SamplerError::from_kind(&kind, pid, RUNTIME_DISPLAY_NAME, message),
            None => SamplerError::AttachFailed(message),
        },
        other => SamplerError::AttachFailed(other.to_string()),
    }
}

fn attach_kind(kind: ErrorKind) -> Option<AttachErrorKind> {
    match kind {
        ErrorKind::ProcessNotFound => Some(AttachErrorKind::ProcessNotFound),
        ErrorKind::UnsupportedRuntime => Some(AttachErrorKind::UnsupportedRuntime),
        ErrorKind::PermissionDenied => Some(AttachErrorKind::PermissionDenied),
        ErrorKind::AttachFailed => Some(AttachErrorKind::AttachFailed),
        ErrorKind::Failed => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::TickThread;

    #[test]
    fn ticks_resolve_frame_ids_leaf_first_and_carry_their_weight() {
        let mut table = FrameTable::default();
        table.extend(vec![
            FrameRecord {
                name: "Main".to_owned(),
                file: Some("/a.cs".to_owned()),
                line: Some(3),
                module: Some("App".to_owned()),
            },
            FrameRecord {
                name: "Hot".to_owned(),
                file: Some("/a.cs".to_owned()),
                line: Some(9),
                module: Some("App".to_owned()),
            },
        ]);
        let batch = table.batch(TickRecord {
            weight_ms: 2.0,
            threads: vec![TickThread {
                tid: 4,
                active: true,
                stack: vec![1, 0, 7],
            }],
        });
        assert_eq!(batch.weight_secs, Some(0.002));
        let names: Vec<&str> = batch
            .samples
            .first()
            .map(|s| s.frames.iter().map(|f| f.name.as_str()).collect())
            .unwrap_or_default();
        assert_eq!(
            names,
            vec!["Hot", "Main"],
            "unknown IDs are dropped, order kept leaf first"
        );
    }

    #[test]
    fn engine_failures_classify_like_every_other_backend() {
        let missing = sampler_error(EngineError::Missing("nope".to_owned()), 1);
        assert_eq!(missing, SamplerError::ToolMissing("nope".to_owned()));
        let gone = sampler_error(
            EngineError::Failed {
                kind: ErrorKind::ProcessNotFound,
                message: "x".to_owned(),
            },
            42,
        );
        assert_eq!(gone, SamplerError::ProcessNotFound(42));
        let other = sampler_error(
            EngineError::Failed {
                kind: ErrorKind::Failed,
                message: "boom".to_owned(),
            },
            42,
        );
        assert_eq!(other, SamplerError::AttachFailed("boom".to_owned()));
    }
}
