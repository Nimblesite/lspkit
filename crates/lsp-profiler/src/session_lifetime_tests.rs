//! Lifetime regressions for the CPU session registry. [MEM-060-CPU-SESSIONS]
//!
//! Every recording holds a clone of its runtime, so the runtime's strong
//! count is the number of recordings the registry still retains.

use std::sync::Arc;
use std::time::Duration;

use super::{CpuSessionManager, MAX_ENDED_SESSIONS};
use crate::runtime::{
    Classification, CpuBackend, FrameFilter, KeepAllFrames, MemoryBackend, ProcessCandidate,
    Runtime,
};
use crate::sampler::{SampleBatch, SamplerHandle};
use crate::wire::{StackFrame, ThreadSample};

/// Sessions a client starts and never stops, far past any retention bound.
const ABANDONED: u32 = 64;

/// A runtime with no backends: the registry is exercised directly.
#[derive(Debug)]
struct Recorded;

impl Runtime for Recorded {
    fn id(&self) -> &'static str {
        "recorded"
    }
    fn display_name(&self) -> &'static str {
        "Recorded"
    }
    fn classify(&self, _candidates: &[ProcessCandidate]) -> Vec<(usize, Classification)> {
        Vec::new()
    }
    fn frame_filter(&self) -> &dyn FrameFilter {
        &KeepAllFrames
    }
    fn cpu(&self) -> Option<&dyn CpuBackend> {
        None
    }
    fn memory(&self) -> Option<&dyn MemoryBackend> {
        None
    }
}

/// One sampled stack, so every recording holds real aggregate data.
fn batch() -> SampleBatch {
    SampleBatch::new(vec![ThreadSample {
        thread_id: 1,
        thread_name: None,
        active: true,
        frames: vec![StackFrame {
            name: "spin".to_owned(),
            file: None,
            line: None,
            module: None,
        }],
    }])
}

/// Start a session on `pid` whose producer sends one batch and finishes.
async fn finished_session(
    manager: &CpuSessionManager,
    runtime: &Arc<dyn Runtime>,
    pid: u32,
    limit: Option<Duration>,
) -> Result<String, String> {
    let parts = SamplerHandle::channel(pid, String::new(), Duration::from_secs(1));
    parts
        .sink
        .sender
        .send(batch())
        .await
        .map_err(|error| error.to_string())?;
    drop(parts.sink);
    drop(parts.done);
    let started = manager
        .adopt(Arc::clone(runtime), parts.handle, 100, limit)
        .await
        .map_err(|error| error.to_string())?;
    Ok(started.session_id)
}

/// Recordings the registry still holds.
fn retained(runtime: &Arc<dyn Runtime>) -> usize {
    Arc::strong_count(runtime) - 1
}

#[tokio::test]
async fn sessions_that_ended_by_duration_and_are_never_stopped_do_not_accumulate(
) -> Result<(), String> {
    let manager = CpuSessionManager::default();
    let runtime: Arc<dyn Runtime> = Arc::new(Recorded);
    for pid in 1..=ABANDONED {
        let _id = finished_session(&manager, &runtime, pid, Some(Duration::ZERO)).await?;
    }
    if retained(&runtime) > MAX_ENDED_SESSIONS {
        return Err(format!(
            "{} ended recordings kept for {ABANDONED} sessions",
            retained(&runtime)
        ));
    }
    if !manager.list().await.is_empty() {
        return Err("an ended session is listed as active".to_owned());
    }
    Ok(())
}

#[tokio::test]
async fn sessions_whose_target_exited_end_and_do_not_accumulate() -> Result<(), String> {
    let manager = CpuSessionManager::default();
    let runtime: Arc<dyn Runtime> = Arc::new(Recorded);
    for pid in 1..=ABANDONED {
        let _id = finished_session(&manager, &runtime, pid, None).await?;
    }
    if retained(&runtime) > MAX_ENDED_SESSIONS {
        return Err(format!(
            "{} dead-target recordings kept for {ABANDONED} sessions",
            retained(&runtime)
        ));
    }
    Ok(())
}

#[tokio::test]
async fn the_newest_ended_recording_is_still_returned_by_stop() -> Result<(), String> {
    let manager = CpuSessionManager::default();
    let runtime: Arc<dyn Runtime> = Arc::new(Recorded);
    let mut last = String::new();
    for pid in 1..=ABANDONED {
        last = finished_session(&manager, &runtime, pid, Some(Duration::ZERO)).await?;
    }
    let recorded = manager
        .stop(&last)
        .await
        .map_err(|error| error.to_string())?;
    if recorded.data.total_samples != 1 {
        return Err(format!(
            "stop returned {} samples, not the 1 recorded",
            recorded.data.total_samples
        ));
    }
    if manager.stop(&last).await.is_ok() {
        return Err("a stopped recording was returned twice".to_owned());
    }
    Ok(())
}
