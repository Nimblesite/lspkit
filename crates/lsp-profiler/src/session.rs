//! CPU sessions. Implements [PROF-CPU-SESSIONS].
//!
//! One session per PID. Each session owns its backend's [`SamplerHandle`] and
//! drains batches into its [`ProfileData`] on demand — on progress ticks,
//! listings, snapshots and stop — through the runtime's frame filter.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::aggregate::{saturating_i64, HotspotConfig, ProfileData};
use crate::error::ProfilerError;
use crate::runtime::Runtime;
use crate::sampler::SamplerHandle;
use crate::wire::{HotFunction, ProfileProgress, ProfileSessionInfo, StartProfileResult};

/// One active CPU session.
struct CpuSession {
    session_id: String,
    runtime: Arc<dyn Runtime>,
    started_at: Instant,
    started_at_iso: String,
    data: ProfileData,
    sampler: SamplerHandle,
    sample_rate: u64,
    include_idle: bool,
    /// The requested duration: sampling ends there, so recorded time does too.
    limit: Option<Duration>,
}

impl CpuSession {
    /// Seconds recorded: the time since start, capped at the requested
    /// duration once the session has ended by itself ([PROF-CPU-SESSIONS]).
    fn recorded_secs(&self) -> f64 {
        let elapsed = self.started_at.elapsed();
        self.limit
            .map_or(elapsed, |limit| elapsed.min(limit))
            .as_secs_f64()
    }

    /// True once a requested duration has elapsed: the session ended by itself
    /// and only its recording remains, kept for the next `stop` ([PROF-CPU-SESSIONS]).
    fn ended(&self) -> bool {
        self.limit
            .is_some_and(|limit| self.started_at.elapsed() >= limit)
    }

    /// Seconds one batch represents when the backend gives no weight.
    fn weight(&self) -> f64 {
        1.0 / f64::from(u32::try_from(self.sample_rate).unwrap_or(u32::MAX).max(1))
    }

    /// Drain every pending batch into the aggregate.
    fn drain(&mut self) {
        let weight = self.weight();
        let mut batches = 0_u64;
        while let Ok(batch) = self.sampler.receiver.try_recv() {
            self.data.ingest(
                &batch,
                weight,
                self.include_idle,
                self.runtime.frame_filter(),
            );
            batches += 1;
        }
        if batches > 0 {
            debug!(session_id = %self.session_id, batches, total = self.data.total_samples, "drained sample batches");
        }
    }

    fn info(&self) -> ProfileSessionInfo {
        ProfileSessionInfo {
            session_id: self.session_id.clone(),
            pid: i64::from(self.sampler.pid),
            runtime: self.runtime.id().to_owned(),
            runtime_version: self.sampler.runtime_version.clone(),
            started_at: self.started_at_iso.clone(),
            sample_count: saturating_i64(self.data.total_samples),
            duration_secs: self.recorded_secs(),
        }
    }

    fn recorded(&self) -> RecordedProfile {
        RecordedProfile {
            session_id: self.session_id.clone(),
            pid: self.sampler.pid,
            runtime: Arc::clone(&self.runtime),
            duration_secs: self.recorded_secs(),
            sample_rate: self.sample_rate,
            data: self.data.clone(),
        }
    }
}

/// Everything recorded by a session, ready for reporting.
#[derive(Debug, Clone)]
pub struct RecordedProfile {
    /// Session identifier.
    pub session_id: String,
    /// Profiled process.
    pub pid: u32,
    /// Runtime that owned the session.
    pub runtime: Arc<dyn Runtime>,
    /// Seconds recorded.
    pub duration_secs: f64,
    /// Samples per second.
    pub sample_rate: u64,
    /// The aggregate.
    pub data: ProfileData,
}

/// Every CPU session of one language server. Clones share the table, so a
/// clone can drive the live-progress task.
#[derive(Clone, Default)]
pub struct CpuSessionManager {
    sessions: Arc<Mutex<HashMap<String, CpuSession>>>,
}

impl std::fmt::Debug for CpuSessionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpuSessionManager").finish_non_exhaustive()
    }
}

impl CpuSessionManager {
    /// Fail when `pid` already has a session.
    ///
    /// # Errors
    ///
    /// Returns [`ProfilerError::AlreadyProfiling`].
    pub async fn ensure_free(&self, pid: u32) -> Result<(), ProfilerError> {
        let sessions = self.sessions.lock().await;
        match sessions.values().find(|s| s.sampler.pid == pid) {
            Some(existing) => Err(ProfilerError::AlreadyProfiling {
                pid,
                session_id: existing.session_id.clone(),
            }),
            None => Ok(()),
        }
    }

    /// Register a freshly attached sampler as a session.
    ///
    /// # Errors
    ///
    /// Returns [`ProfilerError::AlreadyProfiling`] if the PID gained a session meanwhile.
    pub async fn adopt(
        &self,
        runtime: Arc<dyn Runtime>,
        sampler: SamplerHandle,
        sample_rate: u64,
        limit: Option<Duration>,
    ) -> Result<StartProfileResult, ProfilerError> {
        self.ensure_free(sampler.pid).await?;
        let session = CpuSession {
            session_id: mint_id("prof", 0),
            runtime,
            started_at: Instant::now(),
            started_at_iso: rfc3339_now(),
            data: ProfileData::default(),
            sampler,
            sample_rate,
            include_idle: false,
            limit,
        };
        let info = session.info();
        info!(session_id = %info.session_id, pid = info.pid, runtime = %info.runtime, sample_rate, "profiling session started");
        let _ = self
            .sessions
            .lock()
            .await
            .insert(info.session_id.clone(), session);
        Ok(StartProfileResult {
            session_id: info.session_id,
            pid: info.pid,
            runtime: info.runtime,
            runtime_version: info.runtime_version,
            started_at: info.started_at,
        })
    }

    /// Stop a session, wait for its producer's last batch, and return the recording.
    ///
    /// # Errors
    ///
    /// Returns [`ProfilerError::SessionNotFound`] for an unknown ID.
    pub async fn stop(&self, session_id: &str) -> Result<RecordedProfile, ProfilerError> {
        let mut session = self
            .sessions
            .lock()
            .await
            .remove(session_id)
            .ok_or_else(|| ProfilerError::SessionNotFound(session_id.to_owned()))?;
        let (weight, include_idle) = (session.weight(), session.include_idle);
        let (data, runtime) = (&mut session.data, &session.runtime);
        session
            .sampler
            .stop(|batch| data.ingest(&batch, weight, include_idle, runtime.frame_filter()))
            .await;
        session.drain();
        info!(
            session_id,
            total = session.data.total_samples,
            "profiling session stopped"
        );
        Ok(session.recorded())
    }

    /// The recording so far, without stopping.
    ///
    /// # Errors
    ///
    /// Returns [`ProfilerError::SessionNotFound`] for an unknown ID.
    pub async fn snapshot(&self, session_id: &str) -> Result<RecordedProfile, ProfilerError> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| ProfilerError::SessionNotFound(session_id.to_owned()))?;
        session.drain();
        Ok(session.recorded())
    }

    /// Live progress for one session; `None` once it has ended.
    /// Implements [PROF-CPU-PROGRESS].
    pub async fn progress(
        &self,
        session_id: &str,
        hotspots: &HotspotConfig,
    ) -> Option<ProfileProgress> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .filter(|session| !session.ended())?;
        session.drain();
        let info = session.info();
        Some(ProfileProgress {
            session_id: info.session_id,
            sample_count: info.sample_count,
            duration_secs: info.duration_secs,
            top_function: hottest_by_self(&session.data.hot_functions(hotspots)),
        })
    }

    /// Every active session; one past its duration has ended and is not listed.
    pub async fn list(&self) -> Vec<ProfileSessionInfo> {
        let mut sessions = self.sessions.lock().await;
        sessions
            .values_mut()
            .filter(|session| !session.ended())
            .map(|session| {
                session.drain();
                session.info()
            })
            .collect()
    }

    /// Signal every session to stop. Called on shutdown.
    pub async fn stop_all(&self) {
        let mut sessions = self.sessions.lock().await;
        if !sessions.is_empty() {
            info!(count = sessions.len(), "stopping all profiling sessions");
        }
        for (_, session) in sessions.drain() {
            session.sampler.signal_stop();
        }
    }
}

/// A unique identifier: `<prefix>-<8 hex>-<sequence hex>`.
#[must_use]
pub fn mint_id(prefix: &str, sequence: u64) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let tie_break = COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs_low = u32::try_from(now.as_secs() % u64::from(u32::MAX)).unwrap_or(0);
    let mixed = now
        .subsec_nanos()
        .wrapping_mul(2_654_435_761)
        .wrapping_add(secs_low);
    format!(
        "{prefix}-{mixed:08x}-{:x}",
        sequence.wrapping_add(tie_break)
    )
}

/// The current time as RFC 3339 UTC, to the second.
#[must_use]
pub fn rfc3339_now() -> String {
    humantime::format_rfc3339_seconds(SystemTime::now()).to_string()
}

/// The function the program spends its own time in: the most self samples,
/// ties broken by name. Ranked by inclusive samples, the root of every whole
/// stack would always be the hottest [PROF-CPU-PROGRESS].
fn hottest_by_self(functions: &[HotFunction]) -> Option<String> {
    functions
        .iter()
        .max_by(|a, b| {
            a.self_samples
                .cmp(&b.self_samples)
                .then_with(|| b.name.cmp(&a.name))
        })
        .map(|hot| hot.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_now_is_utc_to_the_second() {
        let now = rfc3339_now();
        assert_eq!(now.len(), "2026-10-14T00:00:00Z".len(), "{now}");
        assert!(
            now.ends_with('Z') && now.as_bytes().get(10) == Some(&b'T'),
            "{now}"
        );
    }

    #[test]
    fn minted_ids_are_unique_and_prefixed() {
        let ids: std::collections::HashSet<String> =
            (0..1000).map(|n| mint_id("prof", n)).collect();
        assert_eq!(ids.len(), 1000);
        assert!(ids.iter().all(|id| id.starts_with("prof-")));
    }
}
