//! The runtime contract: what a language plugs in. Implements [PROF-RUNTIME].
//!
//! Everything above this trait — sessions, aggregation, hotspots, exports,
//! heap scoring, the JSON-RPC surface and the editor UI — is shared. A runtime
//! supplies only the parts that genuinely differ per language: how to
//! recognise its processes, how to sample them, which frames are runtime
//! plumbing, and how to read its heap.

use std::ffi::OsString;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;

use crate::memory::{MemoryAction, MemoryError, MemoryPayload};
use crate::sampler::{SamplerError, SamplerHandle};
use crate::wire::{CpuCapabilities, MemoryCapabilities, RuntimeInfo, StackFrame};

/// One process from the OS table, offered to every runtime for recognition.
#[derive(Debug, Clone)]
pub struct ProcessCandidate {
    /// Process ID.
    pub pid: u32,
    /// Parent process ID, when known.
    pub ppid: Option<u32>,
    /// Process name.
    pub name: String,
    /// Executable path, when readable.
    pub exe: Option<PathBuf>,
    /// Argument vector, when readable (`argv[0]` first).
    pub cmd: Vec<OsString>,
    /// Working directory, when readable.
    pub cwd: Option<PathBuf>,
    /// The process belongs to a different OS user.
    pub owned_by_other_user: bool,
    /// The process is a direct child of the language server.
    pub child_of_current: bool,
}

impl ProcessCandidate {
    /// Final path component of the executable, the process name otherwise.
    #[must_use]
    pub fn exe_basename(&self) -> String {
        self.exe.as_deref().and_then(Path::file_name).map_or_else(
            || self.name.clone(),
            |name| name.to_string_lossy().into_owned(),
        )
    }
}

/// A runtime's verdict on one process it recognised.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Classification {
    /// Runtime version, when known.
    pub runtime_version: Option<String>,
    /// Host executable (interpreter, `dotnet`, or the binary itself).
    pub executable: Option<String>,
    /// What the process runs: script, assembly or binary.
    pub entry: Option<String>,
    /// Known launcher or framework.
    pub launcher: Option<String>,
    /// Attaching needs elevation.
    pub requires_elevation: bool,
    /// Why attaching is impossible; `None` when attachable.
    pub unattachable_reason: Option<String>,
}

/// Decides which frames are runtime plumbing rather than user code.
pub trait FrameFilter: Send + Sync + Debug {
    /// Whether `frame` is launcher, debugger, sampler or runtime scaffolding
    /// that must not appear in hotspots or roots of flame graphs.
    fn is_scaffolding(&self, frame: &StackFrame) -> bool;
}

/// A filter that keeps every frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeepAllFrames;

impl FrameFilter for KeepAllFrames {
    fn is_scaffolding(&self, _frame: &StackFrame) -> bool {
        false
    }
}

/// How a CPU session should sample.
#[derive(Debug, Clone, PartialEq)]
pub struct SamplerConfig {
    /// Target process.
    pub pid: u32,
    /// Samples per second.
    pub sample_rate: u64,
    /// Include native frames when supported.
    pub include_native: bool,
    /// Include idle threads.
    pub include_idle: bool,
    /// Stop automatically after this long.
    pub duration: Option<Duration>,
}

/// A CPU sampling backend. Implements [PROF-CPU-BACKEND].
#[async_trait]
pub trait CpuBackend: Send + Sync + Debug {
    /// What this backend can do.
    fn capabilities(&self) -> CpuCapabilities;

    /// Attach to a process and start producing sample batches.
    ///
    /// # Errors
    ///
    /// Returns a [`SamplerError`] naming why attaching failed.
    async fn attach(&self, config: &SamplerConfig) -> Result<SamplerHandle, SamplerError>;

    /// Source of a sampler the editor injects into a paused debuggee, streaming
    /// to `sample_file`. `None` when the runtime has no cooperative sampler.
    ///
    /// # Errors
    ///
    /// Returns a [`SamplerError`] when the script cannot be produced.
    fn cooperative_script(
        &self,
        _sample_file: &Path,
        _sample_rate: u64,
    ) -> Option<Result<String, SamplerError>> {
        None
    }
}

/// Per-session facts a memory backend receives on every call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryContext {
    /// Memory session identifier.
    pub session_id: String,
    /// Tracked process.
    pub pid: u32,
    /// Allocation traceback depth requested at start.
    pub traceback_depth: u32,
    /// Maximum allocation sites per snapshot.
    pub max_sites: usize,
}

/// A query about one type's live objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeQuery {
    /// Type to inspect.
    pub type_name: String,
    /// Maximum objects or graph nodes.
    pub limit: u32,
    /// Maximum retention depth.
    pub max_depth: u32,
}

/// A memory backend. Implements [PROF-MEMORY-BACKEND].
///
/// A *courier* backend answers each step with [`MemoryAction::Evaluate`] and
/// later parses the evaluated output in [`MemoryBackend::parse`]; a *direct*
/// backend captures data itself and answers with [`MemoryAction::Payload`].
#[async_trait]
pub trait MemoryBackend: Send + Sync + Debug {
    /// What this backend can do.
    fn capabilities(&self) -> MemoryCapabilities;

    /// Begin tracking.
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`] when tracking cannot begin.
    async fn start(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError>;

    /// Capture a heap snapshot.
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`] when the snapshot cannot be captured.
    async fn snapshot(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError>;

    /// Compare the heap with the previous snapshot.
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`] when the comparison cannot run.
    async fn diff(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError>;

    /// Build a retention graph for live objects of one type.
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`] when unsupported or failing.
    async fn references(
        &self,
        context: &MemoryContext,
        query: &TypeQuery,
    ) -> Result<MemoryAction, MemoryError>;

    /// List live objects of one type.
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`] when unsupported or failing.
    async fn objects_by_type(
        &self,
        context: &MemoryContext,
        query: &TypeQuery,
    ) -> Result<MemoryAction, MemoryError>;

    /// Force a garbage collection.
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`] when unsupported or failing.
    async fn gc_collect(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError>;

    /// Stop tracking and release backend resources for the session.
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`] when teardown fails.
    async fn stop(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError>;

    /// Parse the output of an evaluated courier script. Async so a backend can
    /// collect a payload its script handed off through a file.
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`] when the output carries no recognised payload.
    async fn parse(
        &self,
        context: &MemoryContext,
        output: &str,
    ) -> Result<MemoryPayload, MemoryError>;
}

/// A language runtime the profiler can drive. Implements [PROF-RUNTIME].
pub trait Runtime: Send + Sync + Debug {
    /// Stable identifier, e.g. `python`.
    fn id(&self) -> &str;

    /// Human-readable name, e.g. `Python`.
    fn display_name(&self) -> &str;

    /// Recognise this runtime's processes among `candidates`, returning the
    /// index of each recognised candidate with its classification.
    fn classify(&self, candidates: &[ProcessCandidate]) -> Vec<(usize, Classification)>;

    /// Frames that are runtime plumbing.
    fn frame_filter(&self) -> &dyn FrameFilter;

    /// The CPU backend, when this runtime has one.
    fn cpu(&self) -> Option<&dyn CpuBackend>;

    /// The memory backend, when this runtime has one.
    fn memory(&self) -> Option<&dyn MemoryBackend>;

    /// Whether a process's `entry` is a source file an editor can open
    /// (a script) rather than a binary or an assembly. Implements [PROF-UI-CAPABILITIES].
    fn source_entries(&self) -> bool {
        false
    }

    /// Wire description of this runtime.
    fn info(&self) -> RuntimeInfo {
        RuntimeInfo {
            id: self.id().to_owned(),
            display_name: self.display_name().to_owned(),
            source_entries: self.source_entries(),
            cpu: self.cpu().map(CpuBackend::capabilities),
            memory: self.memory().map(MemoryBackend::capabilities),
        }
    }
}
