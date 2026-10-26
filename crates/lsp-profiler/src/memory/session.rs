//! Memory sessions. Implements [PROF-MEMORY-SESSION].
//!
//! Holds what one tracking session accumulates across steps — the baseline
//! snapshot, the leak tracker and the timeline — and turns every neutral
//! [`MemoryPayload`] into a scored [`MemoryResult`], whichever way the
//! runtime acquired it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;
use tracing::info;

use super::compare::{compare, from_changes};
use super::diagnostics::{allocation_diagnostics, leak_diagnostics, MemoryHotspotConfig};
use super::leaks::LeakTracker;
use super::timeline::{MemoryTimeline, TREND_POINTS};
use super::{heapprofile, MemoryError, MemoryPayload};
use crate::diagnostics::DiagnosticStyle;
use crate::runtime::{MemoryContext, Runtime};
use crate::wire::{FileDiagnostics, HeapDiff, HeapSnapshot, MemoryOutcome, MemoryResult};

/// Sessions kept at once; the oldest is evicted beyond this.
const MAX_SESSIONS: usize = 32;
/// Allocation sites per snapshot when the request does not say.
pub const DEFAULT_MAX_SITES: usize = 50;
/// Traceback depth when the request does not say.
pub const DEFAULT_TRACEBACK_DEPTH: u32 = 25;

/// One tracking session.
struct MemorySession {
    /// Stable identity and runtime acquisition settings shared across steps.
    context: MemoryContext,
    /// Runtime retained for subsequent parsing and memory operations.
    runtime: Arc<dyn Runtime>,
    /// Start time used for session-age eviction and structured timing logs.
    started_at: Instant,
    /// Previous snapshot against which the next comparison is computed.
    baseline: Option<HeapSnapshot>,
    /// Cross-diff growth history used to assign leak confidence.
    leaks: LeakTracker,
    /// Bounded snapshot history displayed by the memory dashboard.
    timeline: MemoryTimeline,
    /// Monotonic counter that gives snapshots session-unique identifiers.
    snapshot_count: u64,
}

/// How results are scored and where exports go.
#[derive(Debug, Clone)]
pub struct MemorySettings {
    /// Diagnostic branding.
    pub style: DiagnosticStyle,
    /// Allocation thresholds.
    pub hotspots: MemoryHotspotConfig,
    /// Directory for `.heapprofile` exports.
    pub output_dir: PathBuf,
}

/// All memory sessions of one language server.
pub struct MemorySessionManager {
    /// Live session state serialized across concurrent memory requests.
    sessions: Mutex<HashMap<String, MemorySession>>,
    /// Process-local sequence used to mint collision-free session IDs.
    next_seq: AtomicU64,
    /// Shared scoring, diagnostic, and artifact-output configuration.
    settings: MemorySettings,
}

impl std::fmt::Debug for MemorySessionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemorySessionManager")
            .finish_non_exhaustive()
    }
}

impl MemorySessionManager {
    /// An empty manager.
    #[must_use]
    pub fn new(settings: MemorySettings) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            next_seq: AtomicU64::new(0),
            settings,
        }
    }

    /// Open a session for `pid` on `runtime`; returns its context.
    pub async fn open(
        &self,
        runtime: Arc<dyn Runtime>,
        pid: u32,
        traceback_depth: Option<u32>,
    ) -> MemoryContext {
        let sequence = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let context = MemoryContext {
            session_id: crate::session::mint_id("mem", sequence),
            pid,
            traceback_depth: traceback_depth.unwrap_or(DEFAULT_TRACEBACK_DEPTH),
            max_sites: DEFAULT_MAX_SITES,
        };
        info!(session_id = %context.session_id, pid, runtime = runtime.id(), "memory session started");
        let session = MemorySession {
            context: context.clone(),
            runtime,
            started_at: Instant::now(),
            baseline: None,
            leaks: LeakTracker::default(),
            timeline: MemoryTimeline::default(),
            snapshot_count: 0,
        };
        let mut sessions = self.sessions.lock().await;
        let _ = sessions.insert(context.session_id.clone(), session);
        evict_oldest(&mut sessions);
        context
    }

    /// The context and runtime of a session.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::UnknownSession`] for an unknown ID.
    pub async fn lookup(
        &self,
        session_id: &str,
    ) -> Result<(MemoryContext, Arc<dyn Runtime>), MemoryError> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| MemoryError::UnknownSession(session_id.to_owned()))?;
        Ok((session.context.clone(), Arc::clone(&session.runtime)))
    }

    /// Close a session.
    pub async fn close(&self, session_id: &str) {
        let _ = self.sessions.lock().await.remove(session_id);
    }

    /// Parse evaluated courier output with the session's runtime, then score it.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown session or unrecognised output.
    pub async fn ingest(
        &self,
        session_id: &str,
        output: &str,
    ) -> Result<MemoryResult, MemoryError> {
        let (context, runtime) = self.lookup(session_id).await?;
        let backend = runtime
            .memory()
            .ok_or_else(|| MemoryError::Unsupported("memory profiling".to_owned()))?;
        let payload = backend.parse(&context, output).await?;
        self.apply(session_id, payload).await
    }

    /// Score one payload into a result. Implements [PROF-MEMORY-OUTCOME].
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::UnknownSession`] for an unknown ID.
    pub async fn apply(
        &self,
        session_id: &str,
        payload: MemoryPayload,
    ) -> Result<MemoryResult, MemoryError> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| MemoryError::UnknownSession(session_id.to_owned()))?;
        let (outcome, diagnostics) = session.score(payload, &self.settings);
        Ok(MemoryResult {
            memory_session_id: session_id.to_owned(),
            outcome,
            diagnostics,
        })
    }
}

impl MemorySession {
    /// Convert neutral runtime output into a scored outcome and diagnostics.
    fn score(
        &mut self,
        payload: MemoryPayload,
        settings: &MemorySettings,
    ) -> (MemoryOutcome, Vec<FileDiagnostics>) {
        match payload {
            MemoryPayload::Ack => (MemoryOutcome::Ack, Vec::new()),
            MemoryPayload::Snapshot(snapshot) => self.adopt_snapshot(snapshot, settings),
            MemoryPayload::Compare(snapshot) => {
                let snapshot = self.stamp(snapshot);
                let diff = self
                    .baseline
                    .as_ref()
                    .map_or_else(|| from_changes(Vec::new()), |base| compare(base, &snapshot));
                self.timeline.record(&snapshot);
                self.baseline = Some(snapshot);
                self.score_diff(diff, settings)
            }
            MemoryPayload::Diff(diff) => self.score_diff(diff, settings),
            MemoryPayload::Gc(gc) => (MemoryOutcome::Gc { gc }, Vec::new()),
            MemoryPayload::Refs(graph) => (MemoryOutcome::Refs { graph }, Vec::new()),
            MemoryPayload::Objects(objects) => (MemoryOutcome::Objects { objects }, Vec::new()),
        }
    }

    /// Give a snapshot this session's next ID.
    fn stamp(&mut self, mut snapshot: HeapSnapshot) -> HeapSnapshot {
        snapshot.snapshot_id = format!("{}-snap-{}", self.context.session_id, self.snapshot_count);
        self.snapshot_count += 1;
        snapshot
    }

    /// Establish a new baseline, export the snapshot, and update the timeline.
    fn adopt_snapshot(
        &mut self,
        snapshot: HeapSnapshot,
        settings: &MemorySettings,
    ) -> (MemoryOutcome, Vec<FileDiagnostics>) {
        let snapshot = self.stamp(snapshot);
        let diagnostics = allocation_diagnostics(&settings.style, &snapshot, &settings.hotspots);
        self.timeline.record(&snapshot);
        let heapprofile_path = heapprofile::export(&snapshot, &settings.output_dir)
            .map_err(|error| tracing::warn!(%error, "heapprofile export failed"))
            .ok()
            .map(|path| path.display().to_string());
        info!(session_id = %self.context.session_id, sites = snapshot.sites.len(), elapsed = self.started_at.elapsed().as_secs_f64(), "memory snapshot scored");
        self.baseline = Some(snapshot.clone());
        let outcome = MemoryOutcome::Snapshot {
            snapshot,
            timeline: self.timeline.points().to_vec(),
            trending_upward: self.timeline.is_trending_upward(TREND_POINTS),
            heapprofile_path,
        };
        (outcome, diagnostics)
    }

    /// Update leak streaks and derive diagnostics from a computed heap diff.
    fn score_diff(
        &mut self,
        diff: HeapDiff,
        settings: &MemorySettings,
    ) -> (MemoryOutcome, Vec<FileDiagnostics>) {
        let leaks = self.leaks.process_growths(&diff.grown);
        let diagnostics = leak_diagnostics(&settings.style, &leaks, settings.hotspots.max_per_file);
        info!(session_id = %self.context.session_id, growths = diff.grown.len(), "memory diff scored");
        (MemoryOutcome::Diff { diff, leaks }, diagnostics)
    }
}

/// Evict the oldest sessions beyond [`MAX_SESSIONS`].
fn evict_oldest(sessions: &mut HashMap<String, MemorySession>) {
    while sessions.len() > MAX_SESSIONS {
        let oldest = sessions
            .values()
            .min_by_key(|s| s.started_at)
            .map(|s| s.context.session_id.clone());
        match oldest {
            Some(id) => {
                let _ = sessions.remove(&id);
            }
            None => return,
        }
    }
}
