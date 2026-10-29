//! Memory profiling engine. Implements [PROF-MEMORY].
//!
//! A runtime's [`crate::runtime::MemoryBackend`] produces neutral
//! [`MemoryPayload`]s — heap snapshots, diffs, collection results, retention
//! graphs and object lists — either by asking the editor to evaluate a script
//! in the debuggee ([`MemoryAction::Evaluate`]) or by capturing them itself
//! ([`MemoryAction::Payload`]). Everything after that is shared: snapshot
//! comparison, leak-confidence scoring, the timeline, `.heapprofile` export
//! and diagnostics.

pub mod compare;
pub mod diagnostics;
mod gate;
pub mod heapprofile;
pub mod leaks;
pub mod session;
pub mod timeline;

use crate::wire::{AttachErrorKind, GcResult, HeapDiff, HeapSnapshot, ObjectList, RefGraph};

/// What a backend wants done for one memory step.
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryAction {
    /// The editor evaluates `script` in the paused debuggee and posts the
    /// output back to `lspProfiler/memory/ingest`.
    Evaluate {
        /// Source to evaluate.
        script: String,
        /// Text marking the script's output as complete.
        output_marker: String,
    },
    /// The backend captured the data itself.
    Payload(MemoryPayload),
}

/// Neutral memory data, before session scoring.
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryPayload {
    /// Acknowledged with no data.
    Ack,
    /// A snapshot that becomes the new baseline.
    Snapshot(HeapSnapshot),
    /// A diff the runtime computed itself.
    Diff(HeapDiff),
    /// A fresh snapshot to compare against the previous baseline; the session
    /// computes the diff, then adopts it as the new baseline.
    Compare(HeapSnapshot),
    /// A forced collection.
    Gc(GcResult),
    /// A retention graph.
    Refs(RefGraph),
    /// Live objects of one type.
    Objects(ObjectList),
}

/// Why a memory step failed. Implements [PROF-ERRORS].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MemoryError {
    /// No such memory session.
    #[error("unknown memory session: {0}")]
    UnknownSession(String),
    /// The session stopped before the step answered ([PROF-MEMORY-STOP]).
    #[error("memory session {0} stopped before the step finished")]
    Stopped(String),
    /// The runtime does not support the requested step.
    #[error("{0} is not supported by this runtime")]
    Unsupported(String),
    /// A required external tool is missing; the message says how to install it.
    #[error("{0}")]
    ToolMissing(String),
    /// The backend could not reach the process; `kind` classifies why.
    #[error("{message}")]
    Attach {
        /// Classified cause.
        kind: AttachErrorKind,
        /// Backend detail.
        message: String,
    },
    /// The backend failed.
    #[error("{0}")]
    Backend(String),
    /// Evaluated output carried no recognised payload.
    #[error("{0}")]
    Payload(String),
}

/// Bytes as a human-readable string with one decimal (B, KB, MB, GB).
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [(u64, &str); 3] = [(1 << 30, "GB"), (1 << 20, "MB"), (1 << 10, "KB")];
    UNITS.iter().find(|(size, _)| bytes >= *size).map_or_else(
        || format!("{bytes} B"),
        |(size, unit)| format!("{}.{} {unit}", bytes / size, (bytes % size) * 10 / size),
    )
}

/// A wire integer as an unsigned byte count (negative clamps to zero).
#[must_use]
pub fn to_bytes(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
        assert_eq!(format_bytes(248_000_000), "236.5 MB");
    }

    #[test]
    fn negative_wire_sizes_clamp_to_zero() {
        assert_eq!(to_bytes(-5), 0);
        assert_eq!(to_bytes(7), 7);
    }
}
