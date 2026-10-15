//! The JSON-RPC surface. Implements [PROF-SERVICE].
//!
//! A language server owns one [`ProfilerService`] built from its runtimes,
//! routes every `lspProfiler/*` request to [`ProfilerService::handle`], and
//! implements [`ProfilerEvents`] to deliver notifications. The service is
//! transport-agnostic: it takes and returns `serde_json::Value`s.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::aggregate::HotspotConfig;
use crate::diagnostics::DiagnosticStyle;
use crate::error::ProfilerError;
use crate::memory::diagnostics::MemoryHotspotConfig;
use crate::memory::session::{MemorySessionManager, MemorySettings};
use crate::memory::{MemoryAction, MemoryError};
use crate::presets::{resolve, SamplingRequest};
use crate::report::build_report;
use crate::runtime::{MemoryBackend, Runtime, TypeQuery};
use crate::session::CpuSessionManager;
use crate::wire::{
    methods, CooperativeAttachParams, CooperativeScript, CooperativeScriptParams,
    KillProcessParams, KillProcessResult, MemoryIngestParams, MemoryResult, MemorySessionParams,
    MemoryStartParams, MemoryStartResult, MemoryStep, MemoryTypeParams, ProcessList,
    ProcessListParams, ProfileReport, ProfileSessionList, ProfileSessionParams,
    ProfilerCapabilities, StartProfileParams, StartProfileResult,
};
use crate::{cooperative, process};

/// How often live progress is pushed while a CPU session runs.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
/// Default retention-graph node budget.
const DEFAULT_GRAPH_LIMIT: u32 = 200;
/// Default retention-graph depth.
const DEFAULT_GRAPH_DEPTH: u32 = 6;

/// Delivers server-to-client notifications for the host's transport.
#[async_trait]
pub trait ProfilerEvents: Send + Sync + std::fmt::Debug {
    /// Send notification `method` with `params`.
    async fn notify(&self, method: &'static str, params: Value);
}

/// Tunables of a service.
#[derive(Debug, Clone)]
pub struct ProfilerConfig {
    /// Diagnostic branding.
    pub style: DiagnosticStyle,
    /// CPU hotspot thresholds.
    pub hotspots: HotspotConfig,
    /// Memory hotspot thresholds.
    pub memory_hotspots: MemoryHotspotConfig,
    /// Directory for exported artifacts.
    pub output_dir: PathBuf,
}

impl Default for ProfilerConfig {
    fn default() -> Self {
        Self {
            style: DiagnosticStyle::default(),
            hotspots: HotspotConfig::default(),
            memory_hotspots: MemoryHotspotConfig::default(),
            output_dir: std::env::temp_dir().join("lsp-profiler"),
        }
    }
}

struct Inner {
    runtimes: Vec<Arc<dyn Runtime>>,
    cpu: CpuSessionManager,
    memory: MemorySessionManager,
    config: ProfilerConfig,
    events: Arc<dyn ProfilerEvents>,
}

/// The profiler of one language server. Cheap to clone.
#[derive(Clone)]
pub struct ProfilerService {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for ProfilerService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<&str> = self.inner.runtimes.iter().map(|r| r.id()).collect();
        f.debug_struct("ProfilerService")
            .field("runtimes", &ids)
            .finish_non_exhaustive()
    }
}

/// Decode request parameters, treating `null` as an empty object.
fn params<T: DeserializeOwned>(value: Value) -> Result<T, ProfilerError> {
    let value = if value.is_null() {
        Value::Object(serde_json::Map::new())
    } else {
        value
    };
    serde_json::from_value(value).map_err(|err| ProfilerError::InvalidParams(err.to_string()))
}

/// Encode a result.
fn encode<T: Serialize>(value: &T) -> Result<Value, ProfilerError> {
    serde_json::to_value(value)
        .map_err(|err| ProfilerError::Other(format!("cannot encode result: {err}")))
}

/// A wire PID as `u32`.
fn pid_of(pid: i64) -> Result<u32, ProfilerError> {
    u32::try_from(pid)
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| ProfilerError::InvalidParams(format!("invalid pid {pid}")))
}

impl ProfilerService {
    /// A service over `runtimes`. Creates the output directory.
    #[must_use]
    pub fn new(
        runtimes: Vec<Arc<dyn Runtime>>,
        config: ProfilerConfig,
        events: Arc<dyn ProfilerEvents>,
    ) -> Self {
        if let Err(error) = std::fs::create_dir_all(&config.output_dir) {
            warn!(%error, "cannot create the profiler output directory");
        }
        let memory = MemorySessionManager::new(MemorySettings {
            style: config.style.clone(),
            hotspots: config.memory_hotspots.clone(),
            output_dir: config.output_dir.clone(),
        });
        Self {
            inner: Arc::new(Inner {
                runtimes,
                cpu: CpuSessionManager::default(),
                memory,
                config,
                events,
            }),
        }
    }

    /// Whether `method` belongs to this protocol.
    #[must_use]
    pub fn handles(method: &str) -> bool {
        method.starts_with("lspProfiler/")
    }

    /// Dispatch one request. Implements [PROF-WIRE-METHODS].
    ///
    /// # Errors
    ///
    /// Returns a [`ProfilerError`]; [`ProfilerError::code`] gives its JSON-RPC code.
    pub async fn handle(&self, method: &str, raw: Value) -> Result<Value, ProfilerError> {
        debug!(method, "profiler request");
        match method {
            methods::CAPABILITIES => encode(&self.capabilities()),
            methods::PROCESSES => encode(&self.processes(params(raw)?).await?),
            methods::KILL => encode(&self.kill(params(raw)?).await?),
            methods::CPU_START => encode(&self.cpu_start(params(raw)?).await?),
            methods::CPU_STOP => encode(&self.cpu_stop(params(raw)?).await?),
            methods::CPU_SNAPSHOT => encode(&self.cpu_snapshot(params(raw)?).await?),
            methods::CPU_LIST => encode(&ProfileSessionList {
                sessions: self.inner.cpu.list().await,
            }),
            methods::CPU_COOPERATIVE_SCRIPT => encode(&self.cooperative_script(&params(raw)?)?),
            methods::CPU_COOPERATIVE_ATTACH => {
                encode(&self.cooperative_attach(params(raw)?).await?)
            }
            methods::MEMORY_START => encode(&self.memory_start(params(raw)?).await?),
            methods::MEMORY_INGEST => encode(&self.memory_ingest(params(raw)?).await?),
            methods::MEMORY_REFERENCES | methods::MEMORY_OBJECTS_BY_TYPE => {
                encode(&self.memory_type_step(method, params(raw)?).await?)
            }
            other => encode(&self.memory_step(other, params(raw)?).await?),
        }
    }

    /// Every runtime's capabilities.
    #[must_use]
    pub fn capabilities(&self) -> ProfilerCapabilities {
        ProfilerCapabilities {
            runtimes: self.inner.runtimes.iter().map(|r| r.info()).collect(),
        }
    }

    /// Recognised processes. Implements [PROF-PROCESSES].
    ///
    /// # Errors
    ///
    /// Returns an error when the enumeration task fails.
    pub async fn processes(
        &self,
        request: ProcessListParams,
    ) -> Result<ProcessList, ProfilerError> {
        let runtimes = self.inner.runtimes.clone();
        let roots: Vec<PathBuf> = request.workspace_roots.iter().map(PathBuf::from).collect();
        let found = tokio::task::spawn_blocking(move || process::enumerate(&runtimes, &roots))
            .await
            .map_err(|err| ProfilerError::Other(format!("process enumeration failed: {err}")))?;
        Ok(ProcessList {
            processes: found.into_iter().map(|p| p.info).collect(),
        })
    }

    /// Terminate a recognised process. Implements [PROF-PROCESSES-KILL].
    ///
    /// # Errors
    ///
    /// Refuses a PID no runtime recognises.
    pub async fn kill(
        &self,
        request: KillProcessParams,
    ) -> Result<KillProcessResult, ProfilerError> {
        let pid = pid_of(request.pid)?;
        let _owner = self.recognise(pid).await?;
        tokio::task::spawn_blocking(move || process::kill(pid))
            .await
            .map_err(|err| ProfilerError::Other(err.to_string()))?
            .map_err(ProfilerError::Other)?;
        Ok(KillProcessResult {
            pid: request.pid,
            killed: true,
        })
    }

    /// Start a CPU session. Implements [PROF-CPU-START].
    ///
    /// # Errors
    ///
    /// Returns why the runtime could not be found or the sampler not attached.
    pub async fn cpu_start(
        &self,
        request: StartProfileParams,
    ) -> Result<StartProfileResult, ProfilerError> {
        let pid = pid_of(request.pid)?;
        let config = resolve(
            pid,
            &SamplingRequest {
                sample_rate: request.sample_rate.and_then(|r| u64::try_from(r).ok()),
                include_native: request.include_native,
                duration: request
                    .duration_secs
                    .filter(|d| d.is_finite() && *d > 0.0)
                    .map(Duration::from_secs_f64),
                preset: request.preset.clone(),
            },
        )
        .map_err(ProfilerError::InvalidParams)?;
        self.inner.cpu.ensure_free(pid).await?;
        let runtime = self.cpu_runtime(request.runtime.as_deref(), pid).await?;
        let sampler = self.attach(runtime.as_ref(), &config).await?;
        let started = self
            .inner
            .cpu
            .adopt(
                Arc::clone(&runtime),
                sampler,
                config.sample_rate,
                config.duration,
            )
            .await?;
        self.spawn_progress(started.session_id.clone());
        Ok(started)
    }

    /// Attach the runtime's sampler. A backend that cannot read the target's
    /// runtime version gets the one its runtime's classifier resolved for the
    /// process listing, so a session never reports an empty version
    /// ([PROF-CPU-START]).
    async fn attach(
        &self,
        runtime: &dyn Runtime,
        config: &crate::runtime::SamplerConfig,
    ) -> Result<crate::sampler::SamplerHandle, ProfilerError> {
        let backend = runtime.cpu().ok_or_else(|| {
            ProfilerError::NoRuntime(format!("{} has no CPU profiler", runtime.display_name()))
        })?;
        let mut sampler = backend.attach(config).await?;
        if sampler.runtime_version.is_empty() {
            sampler.runtime_version = self.classified_version(config.pid).await;
        }
        Ok(sampler)
    }

    /// The runtime version the classifiers resolved for `pid`, or empty.
    async fn classified_version(&self, pid: u32) -> String {
        let runtimes = self.inner.runtimes.clone();
        tokio::task::spawn_blocking(move || process::recognise(&runtimes, pid))
            .await
            .ok()
            .flatten()
            .and_then(|found| found.info.runtime_version)
            .unwrap_or_default()
    }

    /// Stop a CPU session and report. Implements [PROF-CPU-STOP].
    ///
    /// # Errors
    ///
    /// Returns [`ProfilerError::SessionNotFound`] for an unknown session.
    pub async fn cpu_stop(
        &self,
        request: ProfileSessionParams,
    ) -> Result<ProfileReport, ProfilerError> {
        let recorded = self.inner.cpu.stop(&request.session_id).await?;
        Ok(self.report(&recorded))
    }

    /// Report a CPU session without stopping it. Implements [PROF-CPU-SNAPSHOT].
    ///
    /// # Errors
    ///
    /// Returns [`ProfilerError::SessionNotFound`] for an unknown session.
    pub async fn cpu_snapshot(
        &self,
        request: ProfileSessionParams,
    ) -> Result<ProfileReport, ProfilerError> {
        let recorded = self.inner.cpu.snapshot(&request.session_id).await?;
        Ok(self.report(&recorded))
    }

    fn report(&self, recorded: &crate::session::RecordedProfile) -> ProfileReport {
        let config = &self.inner.config;
        let report = build_report(
            recorded,
            &config.hotspots,
            &config.style,
            &config.output_dir,
        );
        info!(session_id = %report.session_id, samples = report.total_samples, hot_functions = report.hot_functions.len(), "profile reported");
        report
    }

    /// The sampler script for a cooperative session. Implements [PROF-CPU-COOPERATIVE].
    ///
    /// # Errors
    ///
    /// Returns an error when the runtime has no cooperative sampler.
    pub fn cooperative_script(
        &self,
        request: &CooperativeScriptParams,
    ) -> Result<CooperativeScript, ProfilerError> {
        let runtime = self.runtime_by_id(&request.runtime)?;
        let rate = request
            .sample_rate
            .and_then(|r| u64::try_from(r).ok())
            .unwrap_or(crate::presets::DEFAULT_SAMPLE_RATE);
        let sample_file = cooperative::mint_sample_file();
        let script = runtime
            .cpu()
            .and_then(|cpu| cpu.cooperative_script(&sample_file, rate))
            .ok_or_else(|| {
                ProfilerError::NoRuntime(format!(
                    "{} has no cooperative sampler",
                    runtime.display_name()
                ))
            })??;
        Ok(CooperativeScript {
            script,
            sample_file: sample_file.display().to_string(),
        })
    }

    /// Adopt an injected cooperative sampler as a session.
    ///
    /// # Errors
    ///
    /// Returns an error when the sampler never reports or the PID is busy.
    pub async fn cooperative_attach(
        &self,
        request: CooperativeAttachParams,
    ) -> Result<StartProfileResult, ProfilerError> {
        let runtime = self.runtime_by_id(&request.runtime)?;
        let rate = request
            .sample_rate
            .and_then(|r| u64::try_from(r).ok())
            .unwrap_or(crate::presets::DEFAULT_SAMPLE_RATE);
        let sampler = cooperative::tail(PathBuf::from(request.sample_file)).await?;
        let started = self.inner.cpu.adopt(runtime, sampler, rate, None).await?;
        self.spawn_progress(started.session_id.clone());
        Ok(started)
    }

    /// Begin memory tracking. Implements [PROF-MEMORY-START].
    ///
    /// # Errors
    ///
    /// Returns an error when no runtime with memory support recognises the process.
    pub async fn memory_start(
        &self,
        request: MemoryStartParams,
    ) -> Result<MemoryStartResult, ProfilerError> {
        let pid = pid_of(request.pid)?;
        // Refuse a PID no runtime recognises before any session exists, so a
        // refused start never leaves a session for a process that is not there.
        let _owner = self.recognise(pid).await?;
        let runtime = self.memory_runtime(request.runtime.as_deref(), pid).await?;
        let backend = memory_backend(runtime.as_ref())?;
        let depth = request.traceback_depth.and_then(|d| u32::try_from(d).ok());
        let context = self
            .inner
            .memory
            .open(Arc::clone(&runtime), pid, depth)
            .await;
        let action = backend.start(&context).await?;
        Ok(MemoryStartResult {
            memory_session_id: context.session_id.clone(),
            runtime: runtime.id().to_owned(),
            mode: backend.capabilities().mode,
            step: self.to_step(&context.session_id, action).await?,
        })
    }

    async fn memory_step(
        &self,
        method: &str,
        request: MemorySessionParams,
    ) -> Result<MemoryStep, ProfilerError> {
        let (context, runtime) = self.inner.memory.lookup(&request.memory_session_id).await?;
        let backend = memory_backend(runtime.as_ref())?;
        let action = match method {
            methods::MEMORY_SNAPSHOT => backend.snapshot(&context).await?,
            methods::MEMORY_DIFF => backend.diff(&context).await?,
            methods::MEMORY_GC_COLLECT => backend.gc_collect(&context).await?,
            methods::MEMORY_STOP => backend.stop(&context).await?,
            other => {
                return Err(ProfilerError::InvalidParams(format!(
                    "unknown method {other}"
                )))
            }
        };
        let step = self.to_step(&context.session_id, action).await?;
        if method == methods::MEMORY_STOP && matches!(step, MemoryStep::Completed { .. }) {
            self.inner.memory.close(&context.session_id).await;
        }
        Ok(step)
    }

    async fn memory_type_step(
        &self,
        method: &str,
        request: MemoryTypeParams,
    ) -> Result<MemoryStep, ProfilerError> {
        let (context, runtime) = self.inner.memory.lookup(&request.memory_session_id).await?;
        let backend = memory_backend(runtime.as_ref())?;
        let query = TypeQuery {
            type_name: request.type_name,
            limit: request
                .limit
                .and_then(|l| u32::try_from(l).ok())
                .unwrap_or(DEFAULT_GRAPH_LIMIT),
            max_depth: request
                .max_depth
                .and_then(|d| u32::try_from(d).ok())
                .unwrap_or(DEFAULT_GRAPH_DEPTH),
        };
        let action = if method == methods::MEMORY_REFERENCES {
            backend.references(&context, &query).await?
        } else {
            backend.objects_by_type(&context, &query).await?
        };
        self.to_step(&context.session_id, action).await
    }

    /// Score courier output. Implements [PROF-MEMORY-INGEST].
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown session or unrecognised output.
    pub async fn memory_ingest(
        &self,
        request: MemoryIngestParams,
    ) -> Result<MemoryResult, ProfilerError> {
        Ok(self
            .inner
            .memory
            .ingest(&request.memory_session_id, &request.output)
            .await?)
    }

    async fn to_step(
        &self,
        session_id: &str,
        action: MemoryAction,
    ) -> Result<MemoryStep, ProfilerError> {
        Ok(match action {
            MemoryAction::Evaluate {
                script,
                output_marker,
            } => MemoryStep::Evaluate {
                script,
                output_marker,
            },
            MemoryAction::Payload(payload) => MemoryStep::Completed {
                result: self.inner.memory.apply(session_id, payload).await?,
            },
        })
    }

    /// Stop every CPU session. Call on shutdown.
    pub async fn shutdown(&self) {
        self.inner.cpu.stop_all().await;
    }

    fn spawn_progress(&self, session_id: String) {
        let inner = Arc::clone(&self.inner);
        let _task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                let _tick = ticker.tick().await;
                let Some(progress) = inner
                    .cpu
                    .progress(&session_id, &inner.config.hotspots)
                    .await
                else {
                    debug!(%session_id, "progress notifier finished");
                    return;
                };
                match serde_json::to_value(&progress) {
                    Ok(params) => inner.events.notify(methods::CPU_PROGRESS, params).await,
                    Err(error) => warn!(%error, "cannot encode progress"),
                }
            }
        });
    }

    fn runtime_by_id(&self, id: &str) -> Result<Arc<dyn Runtime>, ProfilerError> {
        self.inner
            .runtimes
            .iter()
            .find(|runtime| runtime.id() == id)
            .cloned()
            .ok_or_else(|| ProfilerError::NoRuntime(format!("unknown runtime '{id}'")))
    }

    /// The runtime that recognises `pid`. A PID naming no process is
    /// `PROCESS_NOT_FOUND`; a live process no runtime recognises is
    /// `UNSUPPORTED_RUNTIME` ([PROF-ERRORS]).
    async fn recognise(&self, pid: u32) -> Result<Arc<dyn Runtime>, ProfilerError> {
        let runtimes = self.inner.runtimes.clone();
        let (found, alive) = tokio::task::spawn_blocking(move || {
            (process::recognise(&runtimes, pid), process::exists(pid))
        })
        .await
        .map_err(|err| ProfilerError::Other(err.to_string()))?;
        found
            .and_then(|process| self.inner.runtimes.get(process.runtime_index).cloned())
            .ok_or_else(|| self.unrecognised(pid, alive))
    }

    fn unrecognised(&self, pid: u32, alive: bool) -> ProfilerError {
        if !alive {
            return crate::sampler::SamplerError::ProcessNotFound(pid).into();
        }
        let ids: Vec<&str> = self.inner.runtimes.iter().map(|r| r.id()).collect();
        ProfilerError::NoRuntime(format!(
            "PID {pid} is not recognised by any profiler runtime ({})",
            ids.join(", ")
        ))
    }

    /// The named runtime, the one runtime with the capability, or the one that
    /// recognises the process — in that order.
    async fn pick(
        &self,
        id: Option<&str>,
        pid: u32,
        capable: impl Fn(&dyn Runtime) -> bool,
    ) -> Result<Arc<dyn Runtime>, ProfilerError> {
        if let Some(id) = id {
            return self.runtime_by_id(id);
        }
        let candidates: Vec<&Arc<dyn Runtime>> = self
            .inner
            .runtimes
            .iter()
            .filter(|r| capable(r.as_ref()))
            .collect();
        if let [only] = candidates.as_slice() {
            return Ok(Arc::clone(only));
        }
        self.recognise(pid).await
    }

    async fn cpu_runtime(
        &self,
        id: Option<&str>,
        pid: u32,
    ) -> Result<Arc<dyn Runtime>, ProfilerError> {
        self.pick(id, pid, |r| r.cpu().is_some()).await
    }

    async fn memory_runtime(
        &self,
        id: Option<&str>,
        pid: u32,
    ) -> Result<Arc<dyn Runtime>, ProfilerError> {
        self.pick(id, pid, |r| r.memory().is_some()).await
    }
}

fn memory_backend(runtime: &dyn Runtime) -> Result<&dyn MemoryBackend, ProfilerError> {
    runtime.memory().ok_or_else(|| {
        MemoryError::Unsupported(format!("memory profiling of {}", runtime.display_name())).into()
    })
}
