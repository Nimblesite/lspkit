//! `lsp-profiler-dotnet` — the .NET runtime for `lsp-profiler`. Implements
//! [PROF-DOTNET].
//!
//! Everything language-specific lives here: recognising .NET processes,
//! driving the out-of-process `lsp-profiler-dotnet-engine` for `EventPipe` CPU
//! sampling and heap snapshots, and translating its NDJSON into the neutral
//! sample and heap models. Sessions, aggregation, hotspots, exports and the
//! editor protocol are the core's and never appear here.

pub mod cpu;
pub mod engine;
pub mod memory;
pub mod process;
pub mod protocol;

use std::path::PathBuf;

use lsp_profiler::runtime::{
    Classification, CpuBackend, FrameFilter, KeepAllFrames, MemoryBackend, ProcessCandidate,
    Runtime,
};

/// Stable runtime identifier.
pub const RUNTIME_ID: &str = "dotnet";
/// Human-readable runtime name.
pub const RUNTIME_DISPLAY_NAME: &str = ".NET";

/// Where the engine lives and where snapshot artifacts go.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DotNetConfig {
    /// Explicit engine executable; `None` means discover it ([PROF-DOTNET-ENGINE-LOCATE]).
    pub engine: Option<PathBuf>,
    /// Directory that keeps heap graphs between memory steps.
    pub output_dir: PathBuf,
}

/// The .NET runtime plug-in.
#[derive(Debug)]
pub struct DotNetRuntime {
    /// Supplies `EventPipe` sampling through the shared profiler CPU interface.
    cpu: cpu::DotNetCpu,
    /// Supplies heap capture and graph queries through the shared memory interface.
    memory: memory::DotNetMemory,
}

impl DotNetRuntime {
    /// A runtime driving the engine described by `config`.
    #[must_use]
    pub fn new(config: DotNetConfig) -> Self {
        let engine = engine::Engine::new(config.engine);
        Self {
            cpu: cpu::DotNetCpu::new(engine.clone()),
            memory: memory::DotNetMemory::new(engine, config.output_dir),
        }
    }
}

impl Runtime for DotNetRuntime {
    fn id(&self) -> &str {
        RUNTIME_ID
    }

    fn display_name(&self) -> &str {
        RUNTIME_DISPLAY_NAME
    }

    fn classify(&self, candidates: &[ProcessCandidate]) -> Vec<(usize, Classification)> {
        process::classify_all(candidates)
    }

    fn frame_filter(&self) -> &dyn FrameFilter {
        &KeepAllFrames
    }

    fn cpu(&self) -> Option<&dyn CpuBackend> {
        Some(&self.cpu)
    }

    fn memory(&self) -> Option<&dyn MemoryBackend> {
        Some(&self.memory)
    }
}
