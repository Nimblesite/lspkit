//! The engine's NDJSON messages. Implements [PROF-DOTNET-ENGINE-PROTOCOL].
//!
//! One JSON object per line on the engine's stdout, discriminated by `kind`.
//! Field names are the engine's `snake_case`; enum values keep the engine's
//! `PascalCase` member names.

use serde::Deserialize;

/// Why the engine could not attach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ErrorKind {
    /// The target PID does not exist.
    ProcessNotFound,
    /// The target exposes no .NET diagnostics endpoint.
    UnsupportedRuntime,
    /// The OS refused access.
    PermissionDenied,
    /// The diagnostics channel refused the session.
    AttachFailed,
    /// Any other failure.
    Failed,
}

/// One symbolized frame of the frame table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FrameRecord {
    /// Fully qualified method name.
    pub name: String,
    /// Source file, when symbols allow.
    pub file: Option<String>,
    /// One-based source line, when symbols allow.
    pub line: Option<i64>,
    /// Assembly name.
    pub module: Option<String>,
}

/// One thread's stack at a tick, as frame-table indices, leaf first.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TickThread {
    /// OS thread ID.
    pub tid: i64,
    /// The thread was executing managed code.
    pub active: bool,
    /// Frame IDs, leaf first.
    pub stack: Vec<usize>,
}

/// Every thread sampled at one instant.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TickRecord {
    /// How much wall time the tick represents.
    pub weight_ms: f64,
    /// The sampled threads.
    pub threads: Vec<TickThread>,
}

/// One live type and where it is declared.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TypeRow {
    /// Runtime type name.
    pub type_name: String,
    /// Live instances.
    pub count: i64,
    /// Bytes those instances occupy.
    pub size_bytes: i64,
    /// Declaring source file, when a PDB was found.
    pub file: Option<String>,
    /// One-based line of the first member, when a PDB was found.
    pub line: Option<i64>,
}

/// One node of a retention graph.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RefNodeRecord {
    /// Object address as `0x…`.
    pub id: String,
    /// Runtime type name.
    pub type_name: String,
    /// Root label or address.
    pub label: String,
    /// Object size.
    pub size_bytes: i64,
    /// Distance from the target.
    pub depth: i64,
    /// The node is an instance of the queried type.
    pub is_target: bool,
    /// The node is held directly by a root.
    pub is_root: bool,
    /// Root kind when `is_root`.
    pub root_kind: Option<String>,
}

/// One edge of a retention graph.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RefEdgeRecord {
    /// Holder node ID.
    pub from: String,
    /// Held node ID.
    pub to: String,
    /// Reference label, when known.
    pub label: Option<String>,
}

/// One live object.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ObjectRecord {
    /// Object address as `0x…`.
    pub id: String,
    /// Runtime type name.
    pub type_name: String,
    /// Object size.
    pub size_bytes: i64,
    /// Short label.
    pub label: String,
}

/// Every message the engine writes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EngineMessage {
    /// Attached; sampling has begun.
    Attached {
        /// Target PID.
        pid: u32,
        /// Runtime version when known.
        runtime_version: Option<String>,
    },
    /// New frame-table entries; IDs continue from the previous message.
    Frames {
        /// The entries.
        items: Vec<FrameRecord>,
    },
    /// A batch of ticks.
    Ticks {
        /// The ticks.
        items: Vec<TickRecord>,
    },
    /// Every tick preceding a snapshot request has been delivered.
    Flushed,
    /// Sampling finished.
    Stopped {
        /// Ticks delivered in total.
        tick_count: i64,
    },
    /// The command failed.
    Failure {
        /// Classified cause.
        error_kind: ErrorKind,
        /// Human-readable detail.
        message: String,
    },
    /// Live-heap summary of one snapshot.
    Snapshot {
        /// The heap graph file kept for later steps.
        graph_path: String,
        /// Bytes of live objects.
        live_bytes: i64,
        /// Count of live objects.
        live_objects: i64,
        /// Bytes the runtime reported for the heap.
        heap_bytes: i64,
        /// gen0, gen1, gen2, large, pinned.
        generation_bytes: Vec<i64>,
        /// Largest types first.
        types: Vec<TypeRow>,
    },
    /// Who keeps a type alive.
    Refs {
        /// Graph nodes.
        nodes: Vec<RefNodeRecord>,
        /// Graph edges.
        edges: Vec<RefEdgeRecord>,
        /// The node budget was exhausted.
        truncated: bool,
    },
    /// Live objects of one type.
    Objects {
        /// The queried type.
        type_name: String,
        /// Live instances in total.
        total_count: i64,
        /// Bytes of those instances.
        total_bytes: i64,
        /// The listed instances.
        items: Vec<ObjectRecord>,
        /// Largest types overall.
        type_summary: Vec<TypeRow>,
    },
    /// Result of an induced collection.
    Gc {
        /// A collection completed.
        observed: bool,
        /// Heap bytes after it.
        heap_bytes_after: i64,
        /// gen0, gen1, gen2, large, pinned.
        generation_bytes: Vec<i64>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_messages_carry_pascal_case_kinds() {
        let parsed: EngineMessage = serde_json::from_str(
            r#"{"kind":"failure","error_kind":"ProcessNotFound","message":"gone"}"#,
        )
        .unwrap_or(EngineMessage::Stopped { tick_count: -1 });
        assert_eq!(
            parsed,
            EngineMessage::Failure {
                error_kind: ErrorKind::ProcessNotFound,
                message: "gone".to_owned()
            }
        );
    }

    #[test]
    fn ticks_carry_frame_ids_leaf_first() {
        let parsed: EngineMessage =
            serde_json::from_str(r#"{"kind":"ticks","items":[{"weight_ms":1.5,"threads":[{"tid":7,"active":true,"stack":[2,0]}]}]}"#)
                .unwrap_or(EngineMessage::Stopped { tick_count: -1 });
        assert!(
            matches!(parsed, EngineMessage::Ticks { .. }),
            "expected ticks: {parsed:?}"
        );
        let EngineMessage::Ticks { items } = parsed else {
            return;
        };
        assert_eq!(
            items.first().map(|t| (t.weight_ms, t.threads.len())),
            Some((1.5, 1))
        );
        assert_eq!(
            items
                .first()
                .and_then(|t| t.threads.first())
                .map(|t| t.stack.clone()),
            Some(vec![2, 0])
        );
    }
}
