//! `lsp-profiler` — a language-neutral CPU and memory profiling engine for
//! language servers. Implements [PROF].
//!
//! A language plugs in a [`runtime::Runtime`] — how to recognise its
//! processes, sample their stacks, filter runtime plumbing and read the heap.
//! Everything else is shared by every language: sessions, aggregation,
//! hotspots and their diagnostics, speedscope / flame-graph / V8 exports,
//! heap snapshots, leak scoring, the timeline, and one JSON-RPC protocol
//! ([`service::ProfilerService`]) that one editor UI drives for all of them.
//!
//! The wire types in [`wire`] are generated from `docs/models/profiler.td`.

pub mod aggregate;
pub mod commands;
pub mod cooperative;
pub mod diagnostics;
pub mod error;
pub mod export;
pub mod memory;
pub mod ndjson;
pub mod paths;
pub mod presets;
pub mod process;
pub mod report;
pub mod runtime;
pub mod sampler;
pub mod service;
pub mod session;
/// Wire models and method names, generated from `docs/models/profiler.td`.
#[expect(
    clippy::struct_excessive_bools,
    reason = "capability and process flags are independent booleans of the wire protocol"
)]
#[path = "wire_generated.rs"]
pub mod wire;

pub use error::ProfilerError;
pub use runtime::{
    Classification, CpuBackend, FrameFilter, KeepAllFrames, MemoryBackend, MemoryContext,
    ProcessCandidate, Runtime, SamplerConfig, TypeQuery,
};
pub use sampler::{SampleBatch, SamplerError, SamplerHandle, SamplerParts, SamplerSink};
pub use service::{ProfilerConfig, ProfilerEvents, ProfilerService};
