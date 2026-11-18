//! Lifetime regressions for the service's background tasks. [PROF-CPU-PROGRESS]
//!
//! The service is the only owner of its events transport, so once the service
//! is dropped the test's handle must be the last strong reference.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use super::{ProfilerConfig, ProfilerEvents, ProfilerService};
use crate::runtime::{
    Classification, CpuBackend, FrameFilter, KeepAllFrames, MemoryBackend, ProcessCandidate,
    Runtime, SamplerConfig,
};
use crate::sampler::{SamplerError, SamplerHandle, SamplerSink};
use crate::wire::{CpuCapabilities, StartProfileParams};

/// Runtime and backend identity.
const RUNTIME_ID: &str = "live";
/// A non-empty version, so attaching never consults the process table.
const VERSION: &str = "1.0";
/// Bound on how long a stop waits for the (never finishing) producer.
const FINISH: Duration = Duration::from_millis(10);

/// A backend whose producers never finish: it keeps every sink alive.
#[derive(Debug, Default)]
struct Live {
    /// Producer halves, held so each session stays recording.
    sinks: Mutex<Vec<(SamplerSink, tokio::sync::oneshot::Sender<()>)>>,
}

#[async_trait]
impl CpuBackend for Live {
    fn capabilities(&self) -> CpuCapabilities {
        CpuCapabilities {
            live_progress: true,
            line_level: false,
            native_frames: false,
            cooperative: false,
        }
    }
    async fn attach(&self, config: &SamplerConfig) -> Result<SamplerHandle, SamplerError> {
        let parts = SamplerHandle::channel(config.pid, VERSION.to_owned(), FINISH);
        self.sinks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((parts.sink, parts.done));
        Ok(parts.handle)
    }
}

/// A runtime exposing only the [`Live`] backend.
#[derive(Debug, Default)]
struct LiveRuntime {
    /// The CPU backend.
    backend: Live,
}

impl Runtime for LiveRuntime {
    fn id(&self) -> &'static str {
        RUNTIME_ID
    }
    fn display_name(&self) -> &'static str {
        RUNTIME_ID
    }
    fn classify(&self, _candidates: &[ProcessCandidate]) -> Vec<(usize, Classification)> {
        Vec::new()
    }
    fn frame_filter(&self) -> &dyn FrameFilter {
        &KeepAllFrames
    }
    fn cpu(&self) -> Option<&dyn CpuBackend> {
        Some(&self.backend)
    }
    fn memory(&self) -> Option<&dyn MemoryBackend> {
        None
    }
}

/// Events sink that discards notifications.
#[derive(Debug)]
struct Discard;

#[async_trait]
impl ProfilerEvents for Discard {
    async fn notify(&self, _method: &'static str, _params: Value) {}
}

/// Start a recording with no duration on this process.
fn open_ended() -> StartProfileParams {
    StartProfileParams {
        pid: i64::from(std::process::id()),
        runtime: Some(RUNTIME_ID.to_owned()),
        sample_rate: None,
        include_native: None,
        duration_secs: None,
        preset: None,
    }
}

#[tokio::test]
async fn an_abandoned_open_ended_session_does_not_keep_a_dropped_service_alive(
) -> Result<(), String> {
    let events: Arc<dyn ProfilerEvents> = Arc::new(Discard);
    let config = ProfilerConfig {
        output_dir: std::env::temp_dir().join("lsp-profiler-lifetime"),
        ..ProfilerConfig::default()
    };
    let service = ProfilerService::new(
        vec![Arc::new(LiveRuntime::default())],
        config,
        Arc::clone(&events),
    );
    let _started = service
        .cpu_start(open_ended())
        .await
        .map_err(|error| error.to_string())?;
    drop(service);
    let held = Arc::strong_count(&events) - 1;
    if held != 0 {
        return Err(format!(
            "the progress task still holds the service: {held} transport reference(s) left"
        ));
    }
    Ok(())
}
