//! Lifetime regressions for memory sessions. [PROF-MEMORY-SESSION]
//!
//! A backend keeps per-session state (the .NET backend: a map entry and the
//! `heap-<id>` graph directory) until its `stop` runs. Every way a session
//! leaves the manager must reach that `stop`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use async_trait::async_trait;

use super::{MemorySessionManager, MemorySettings, MAX_SESSIONS};
use crate::diagnostics::DiagnosticStyle;
use crate::memory::diagnostics::MemoryHotspotConfig;
use crate::memory::{MemoryAction, MemoryError, MemoryPayload};
use crate::runtime::{
    Classification, CpuBackend, FrameFilter, KeepAllFrames, MemoryBackend, MemoryContext,
    ProcessCandidate, Runtime, TypeQuery,
};
use crate::wire::{MemoryCapabilities, MemoryMode};

/// Runtime identity.
const RUNTIME_ID: &str = "holding";
/// Sessions a shutdown test opens.
const SHUTDOWN_SESSIONS: usize = 3;

/// A backend holding one marker per started session until `stop`.
#[derive(Debug, Default)]
struct Holding {
    /// Per-session backend state.
    sessions: Mutex<HashMap<String, Arc<()>>>,
}

impl Holding {
    /// A weak view of a started session's backend state.
    fn marker(&self, session_id: &str) -> Option<Weak<()>> {
        self.lock().get(session_id).map(Arc::downgrade)
    }

    /// The state map.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<()>>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The answer to every step.
const fn ack() -> MemoryAction {
    MemoryAction::Payload(MemoryPayload::Ack)
}

#[async_trait]
impl MemoryBackend for Holding {
    fn capabilities(&self) -> MemoryCapabilities {
        MemoryCapabilities {
            mode: MemoryMode::Direct,
            allocation_sites: false,
            references: false,
            objects_by_type: false,
            gc_collect: false,
        }
    }
    async fn start(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        let _ = self.lock().insert(context.session_id.clone(), Arc::new(()));
        Ok(ack())
    }
    async fn snapshot(&self, _context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        Ok(ack())
    }
    async fn diff(&self, _context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        Ok(ack())
    }
    async fn references(
        &self,
        _context: &MemoryContext,
        _query: &TypeQuery,
    ) -> Result<MemoryAction, MemoryError> {
        Ok(ack())
    }
    async fn objects_by_type(
        &self,
        _context: &MemoryContext,
        _query: &TypeQuery,
    ) -> Result<MemoryAction, MemoryError> {
        Ok(ack())
    }
    async fn gc_collect(&self, _context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        Ok(ack())
    }
    async fn stop(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        let _ = self.lock().remove(&context.session_id);
        Ok(ack())
    }
    async fn parse(
        &self,
        _context: &MemoryContext,
        _output: &str,
    ) -> Result<MemoryPayload, MemoryError> {
        Ok(MemoryPayload::Ack)
    }
}

/// A runtime exposing only the [`Holding`] backend.
#[derive(Debug, Default)]
struct HoldingRuntime {
    /// The memory backend.
    backend: Holding,
}

impl Runtime for HoldingRuntime {
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
        None
    }
    fn memory(&self) -> Option<&dyn MemoryBackend> {
        Some(&self.backend)
    }
}

/// A manager with default scoring.
fn manager() -> MemorySessionManager {
    MemorySessionManager::new(MemorySettings {
        style: DiagnosticStyle::default(),
        hotspots: MemoryHotspotConfig::default(),
        output_dir: std::env::temp_dir().join("lsp-profiler-memory-lifetime"),
    })
}

/// Open and start `count` sessions as the service does; returns their markers.
async fn start_sessions(
    manager: &MemorySessionManager,
    runtime: &Arc<HoldingRuntime>,
    count: usize,
) -> Result<Vec<Weak<()>>, String> {
    let mut markers = Vec::with_capacity(count);
    for _ in 0..count {
        let shared: Arc<dyn Runtime> = Arc::<HoldingRuntime>::clone(runtime);
        let context = manager.open(shared, std::process::id(), None).await;
        let _ = runtime
            .backend
            .start(&context)
            .await
            .map_err(|e| e.to_string())?;
        markers.push(
            runtime
                .backend
                .marker(&context.session_id)
                .ok_or("the backend did not record the session")?,
        );
    }
    Ok(markers)
}

/// How many markers still have an owner.
fn alive(markers: &[Weak<()>]) -> usize {
    markers
        .iter()
        .filter(|marker| marker.strong_count() > 0)
        .count()
}

#[tokio::test]
async fn an_evicted_memory_session_is_stopped_in_its_backend() -> Result<(), String> {
    let manager = manager();
    let runtime = Arc::new(HoldingRuntime::default());
    let markers = start_sessions(&manager, &runtime, MAX_SESSIONS + 1).await?;
    let held = alive(&markers);
    if held != MAX_SESSIONS {
        return Err(format!(
            "{held} backend sessions alive for {MAX_SESSIONS} open sessions: eviction never reached the backend's stop"
        ));
    }
    Ok(())
}

#[tokio::test]
async fn stopping_every_memory_session_releases_every_backend_session() -> Result<(), String> {
    let manager = manager();
    let runtime = Arc::new(HoldingRuntime::default());
    let markers = start_sessions(&manager, &runtime, SHUTDOWN_SESSIONS).await?;
    manager.stop_all().await;
    let held = alive(&markers);
    if held != 0 {
        return Err(format!("{held} backend sessions alive after stop_all"));
    }
    Ok(())
}
