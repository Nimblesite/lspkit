//! The memory half of the JSON-RPC surface. Implements [PROF-MEMORY-START],
//! [PROF-MEMORY-STEP], [PROF-MEMORY-INGEST] and [PROF-MEMORY-STOP].
//!
//! Every step but `stop` runs under its session's admission, so `stop` can end
//! the steps in flight before it releases the session.

use std::sync::Arc;

use super::{pid_of, ProfilerService};
use crate::error::ProfilerError;
use crate::memory::{MemoryAction, MemoryError};
use crate::runtime::{MemoryBackend, MemoryContext, Runtime, TypeQuery};
use crate::wire::{
    methods, MemoryIngestParams, MemoryResult, MemorySessionParams, MemoryStartParams,
    MemoryStartResult, MemoryStep, MemoryTypeParams,
};

/// Default retention-graph node budget.
const DEFAULT_GRAPH_LIMIT: u32 = 200;
/// Default retention-graph depth.
const DEFAULT_GRAPH_DEPTH: u32 = 6;

impl ProfilerService {
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

    /// Dispatch a parameter-free memory operation and normalize its next step.
    pub(super) async fn memory_step(
        &self,
        method: &str,
        request: MemorySessionParams,
    ) -> Result<MemoryStep, ProfilerError> {
        if method == methods::MEMORY_STOP {
            return self.memory_stop(&request.memory_session_id).await;
        }
        let (context, runtime, pass) = self.inner.memory.enter(&request.memory_session_id).await?;
        let backend = memory_backend(runtime.as_ref())?;
        pass.run(async {
            let action = parameterless_step(backend, method, &context).await?;
            self.to_step(&context.session_id, action).await
        })
        .await
    }

    /// End every step in flight, then release the session. Implements
    /// [PROF-MEMORY-STOP].
    async fn memory_stop(&self, session_id: &str) -> Result<MemoryStep, ProfilerError> {
        self.inner.memory.stop_steps(session_id).await;
        let (context, runtime) = self.inner.memory.lookup(session_id).await?;
        let backend = memory_backend(runtime.as_ref())?;
        let step = self
            .to_step(session_id, backend.stop(&context).await?)
            .await?;
        if matches!(step, MemoryStep::Completed { .. }) {
            self.inner.memory.close(session_id).await;
        }
        Ok(step)
    }

    /// Run a bounded type query for references or live-object inspection.
    pub(super) async fn memory_type_step(
        &self,
        method: &str,
        request: MemoryTypeParams,
    ) -> Result<MemoryStep, ProfilerError> {
        let (context, runtime, pass) = self.inner.memory.enter(&request.memory_session_id).await?;
        let backend = memory_backend(runtime.as_ref())?;
        let query = type_query(request);
        pass.run(async {
            let action = if method == methods::MEMORY_REFERENCES {
                backend.references(&context, &query).await?
            } else {
                backend.objects_by_type(&context, &query).await?
            };
            self.to_step(&context.session_id, action).await
        })
        .await
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

    /// Convert runtime instructions into evaluation or a scored completed result.
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

    /// Select a runtime with memory support using the standard resolution order.
    async fn memory_runtime(
        &self,
        id: Option<&str>,
        pid: u32,
    ) -> Result<Arc<dyn Runtime>, ProfilerError> {
        self.pick(id, pid, |r| r.memory().is_some()).await
    }
}

/// Run the parameter-free step `method` on `backend`.
async fn parameterless_step(
    backend: &dyn MemoryBackend,
    method: &str,
    context: &MemoryContext,
) -> Result<MemoryAction, ProfilerError> {
    Ok(match method {
        methods::MEMORY_SNAPSHOT => backend.snapshot(context).await?,
        methods::MEMORY_DIFF => backend.diff(context).await?,
        methods::MEMORY_GC_COLLECT => backend.gc_collect(context).await?,
        other => {
            return Err(ProfilerError::InvalidParams(format!(
                "unknown method {other}"
            )))
        }
    })
}

/// The type query a references or objects request asks for, with the default budgets.
fn type_query(request: MemoryTypeParams) -> TypeQuery {
    TypeQuery {
        type_name: request.type_name,
        limit: request
            .limit
            .and_then(|l| u32::try_from(l).ok())
            .unwrap_or(DEFAULT_GRAPH_LIMIT),
        max_depth: request
            .max_depth
            .and_then(|d| u32::try_from(d).ok())
            .unwrap_or(DEFAULT_GRAPH_DEPTH),
    }
}

/// Require memory support after runtime selection, with a classified error.
fn memory_backend(runtime: &dyn Runtime) -> Result<&dyn MemoryBackend, ProfilerError> {
    runtime.memory().ok_or_else(|| {
        MemoryError::Unsupported(format!("memory profiling of {}", runtime.display_name())).into()
    })
}
