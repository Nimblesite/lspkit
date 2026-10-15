//! Heap snapshots through the engine. Implements [PROF-DOTNET-MEMORY].
//!
//! A direct backend: every step runs one engine command and answers with the
//! neutral payload. A snapshot keeps its heap graph in the output directory
//! so `references` and `objects` re-open it instead of touching the target
//! again; the session's graphs are removed when it stops.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use lsp_profiler::memory::{MemoryAction, MemoryError, MemoryPayload};
use lsp_profiler::runtime::{MemoryBackend, MemoryContext, TypeQuery};
use lsp_profiler::wire::{
    AllocationSite, GcResult, HeapSnapshot, MemoryCapabilities, MemoryMode, ObjectList,
    ObjectSample, RefEdge, RefGraph, RefNode, TypeStat,
};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::engine::{Engine, EngineError};
use crate::protocol::{EngineMessage, ObjectRecord, RefEdgeRecord, RefNodeRecord, TypeRow};

/// What one session keeps between steps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct SessionState {
    /// Heap graph of the latest snapshot.
    graph: Option<PathBuf>,
    /// Snapshots taken so far.
    snapshots: u32,
    /// Heap bytes the latest snapshot or collection reported.
    heap_bytes: Option<i64>,
}

/// The .NET memory backend.
#[derive(Debug)]
pub struct DotNetMemory {
    engine: Engine,
    output_dir: PathBuf,
    sessions: Mutex<HashMap<String, SessionState>>,
}

impl DotNetMemory {
    /// A backend driving `engine`, keeping graphs under `output_dir`.
    #[must_use]
    pub fn new(engine: Engine, output_dir: PathBuf) -> Self {
        Self {
            engine,
            output_dir,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    async fn run(&self, args: Vec<String>) -> Result<EngineMessage, MemoryError> {
        self.engine.run(&args).await.map_err(memory_error)
    }

    async fn graph_of(&self, session_id: &str) -> Result<PathBuf, MemoryError> {
        let sessions = self.sessions.lock().await;
        sessions
            .get(session_id)
            .and_then(|state| state.graph.clone())
            .ok_or_else(|| {
                MemoryError::Backend("take a snapshot before inspecting the heap".to_owned())
            })
    }

    async fn capture(&self, context: &MemoryContext) -> Result<HeapSnapshot, MemoryError> {
        let graph = self.next_graph_path(&context.session_id).await?;
        let args = vec![
            "snapshot".to_owned(),
            "--pid".to_owned(),
            context.pid.to_string(),
            "--graph".to_owned(),
            graph.display().to_string(),
            "--max-types".to_owned(),
            context.max_sites.to_string(),
        ];
        let EngineMessage::Snapshot {
            live_bytes,
            live_objects,
            heap_bytes,
            generation_bytes,
            types,
            ..
        } = self.run(args).await?
        else {
            return Err(MemoryError::Payload(
                "the engine answered a snapshot with another message".to_owned(),
            ));
        };
        self.remember(&context.session_id, |state| {
            state.graph = Some(graph.clone());
            state.heap_bytes = Some(heap_bytes);
        })
        .await;
        Ok(HeapSnapshot {
            snapshot_id: String::new(),
            current_bytes: live_bytes,
            peak_bytes: heap_bytes,
            object_count: live_objects,
            gc_generations: generation_bytes,
            sites: types.into_iter().map(site).collect(),
        })
    }

    async fn next_graph_path(&self, session_id: &str) -> Result<PathBuf, MemoryError> {
        let dir = self.output_dir.join(format!("heap-{session_id}"));
        tokio::fs::create_dir_all(&dir).await.map_err(|error| {
            MemoryError::Backend(format!("cannot create {}: {error}", dir.display()))
        })?;
        let mut sessions = self.sessions.lock().await;
        let state = sessions.entry(session_id.to_owned()).or_default();
        state.snapshots += 1;
        Ok(dir.join(format!("snapshot-{}.heapgraph.json", state.snapshots)))
    }

    async fn remember(&self, session_id: &str, update: impl FnOnce(&mut SessionState)) {
        let mut sessions = self.sessions.lock().await;
        update(sessions.entry(session_id.to_owned()).or_default());
    }
}

#[async_trait]
impl MemoryBackend for DotNetMemory {
    fn capabilities(&self) -> MemoryCapabilities {
        MemoryCapabilities {
            mode: MemoryMode::Direct,
            allocation_sites: false,
            references: true,
            objects_by_type: true,
            gc_collect: true,
        }
    }

    async fn start(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        self.remember(&context.session_id, |_| {}).await;
        Ok(MemoryAction::Payload(MemoryPayload::Ack))
    }

    async fn snapshot(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        Ok(MemoryAction::Payload(MemoryPayload::Snapshot(
            self.capture(context).await?,
        )))
    }

    async fn diff(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        Ok(MemoryAction::Payload(MemoryPayload::Compare(
            self.capture(context).await?,
        )))
    }

    async fn references(
        &self,
        context: &MemoryContext,
        query: &TypeQuery,
    ) -> Result<MemoryAction, MemoryError> {
        let graph = self.graph_of(&context.session_id).await?;
        let args = graph_args(
            "refs",
            &graph,
            &query.type_name,
            &[
                ("--max-depth", query.max_depth),
                ("--max-nodes", query.limit),
            ],
        );
        let EngineMessage::Refs {
            nodes,
            edges,
            truncated,
        } = self.run(args).await?
        else {
            return Err(MemoryError::Payload(
                "the engine answered refs with another message".to_owned(),
            ));
        };
        Ok(MemoryAction::Payload(MemoryPayload::Refs(RefGraph {
            nodes: nodes.into_iter().map(ref_node).collect(),
            edges: edges.into_iter().map(ref_edge).collect(),
            cycles: Vec::new(),
            truncated,
        })))
    }

    async fn objects_by_type(
        &self,
        context: &MemoryContext,
        query: &TypeQuery,
    ) -> Result<MemoryAction, MemoryError> {
        let graph = self.graph_of(&context.session_id).await?;
        let args = graph_args(
            "objects",
            &graph,
            &query.type_name,
            &[("--limit", query.limit)],
        );
        let EngineMessage::Objects {
            type_name,
            total_count,
            total_bytes,
            items,
            type_summary,
        } = self.run(args).await?
        else {
            return Err(MemoryError::Payload(
                "the engine answered objects with another message".to_owned(),
            ));
        };
        Ok(MemoryAction::Payload(MemoryPayload::Objects(ObjectList {
            type_name,
            total_count,
            total_bytes,
            objects: items.into_iter().map(object).collect(),
            type_summary: type_summary.into_iter().map(type_stat).collect(),
        })))
    }

    async fn gc_collect(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        let args = vec!["gc".to_owned(), "--pid".to_owned(), context.pid.to_string()];
        let EngineMessage::Gc {
            observed,
            heap_bytes_after,
            ..
        } = self.run(args).await?
        else {
            return Err(MemoryError::Payload(
                "the engine answered gc with another message".to_owned(),
            ));
        };
        let mut freed = 0;
        self.remember(&context.session_id, |state| {
            freed = state
                .heap_bytes
                .map_or(0, |before| (before - heap_bytes_after).max(0));
            state.heap_bytes = Some(heap_bytes_after);
        })
        .await;
        Ok(MemoryAction::Payload(MemoryPayload::Gc(GcResult {
            collected: i64::from(observed),
            uncollectable_count: 0,
            freed_bytes: freed,
            uncollectable: Vec::new(),
        })))
    }

    async fn stop(&self, context: &MemoryContext) -> Result<MemoryAction, MemoryError> {
        let removed = self.sessions.lock().await.remove(&context.session_id);
        if removed.is_some() {
            let dir = self.output_dir.join(format!("heap-{}", context.session_id));
            if let Err(error) = tokio::fs::remove_dir_all(&dir).await {
                debug!(%error, dir = %dir.display(), "no heap graphs to remove");
            }
        }
        Ok(MemoryAction::Payload(MemoryPayload::Ack))
    }

    async fn parse(
        &self,
        _context: &MemoryContext,
        _output: &str,
    ) -> Result<MemoryPayload, MemoryError> {
        Err(MemoryError::Unsupported(
            "the .NET backend captures directly and evaluates nothing".to_owned(),
        ))
    }
}

fn graph_args(command: &str, graph: &Path, type_name: &str, limits: &[(&str, u32)]) -> Vec<String> {
    let mut args = vec![
        command.to_owned(),
        "--graph".to_owned(),
        graph.display().to_string(),
        "--type".to_owned(),
        type_name.to_owned(),
    ];
    for (flag, value) in limits {
        args.push((*flag).to_owned());
        args.push(value.to_string());
    }
    args
}

/// A type row as the neutral allocation site: the type's declaration stands in for an allocation line.
fn site(row: TypeRow) -> AllocationSite {
    AllocationSite {
        file: row.file,
        line: row.line,
        type_name: Some(row.type_name),
        size_bytes: row.size_bytes,
        count: row.count,
        traceback: Vec::new(),
    }
}

fn type_stat(row: TypeRow) -> TypeStat {
    TypeStat {
        type_name: row.type_name,
        count: row.count,
        size_bytes: row.size_bytes,
    }
}

fn ref_node(node: RefNodeRecord) -> RefNode {
    RefNode {
        id: node.id,
        type_name: node.type_name,
        label: node.label,
        size_bytes: node.size_bytes,
        retained_bytes: None,
        depth: node.depth,
        is_target: node.is_target,
        is_root: node.is_root,
        root_kind: node.root_kind,
    }
}

fn ref_edge(edge: RefEdgeRecord) -> RefEdge {
    RefEdge {
        from: edge.from,
        to: edge.to,
        label: edge.label,
    }
}

fn object(item: ObjectRecord) -> ObjectSample {
    ObjectSample {
        id: item.id,
        type_name: item.type_name,
        size_bytes: item.size_bytes,
        refcount: None,
        label: item.label,
    }
}

fn memory_error(error: EngineError) -> MemoryError {
    match error {
        EngineError::Missing(message) => MemoryError::ToolMissing(message),
        EngineError::Failed { message, .. } => MemoryError::Backend(message),
        other => {
            warn!(%other, "the .NET profiling engine failed");
            MemoryError::Backend(other.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_rows_become_sites_anchored_at_the_declaration() {
        let row = TypeRow {
            type_name: "App.Leak".to_owned(),
            count: 3,
            size_bytes: 96,
            file: Some("/a.cs".to_owned()),
            line: Some(7),
        };
        let site = site(row);
        assert_eq!(
            (
                site.file.as_deref(),
                site.line,
                site.type_name.as_deref(),
                site.count,
                site.size_bytes
            ),
            (Some("/a.cs"), Some(7), Some("App.Leak"), 3, 96)
        );
        assert_eq!(site.traceback.len(), 0);
    }

    #[test]
    fn graph_commands_pass_the_type_and_limits() {
        let args = graph_args(
            "refs",
            Path::new("/g.json"),
            "App.Leak",
            &[("--max-depth", 4), ("--max-nodes", 50)],
        );
        assert_eq!(
            args,
            vec![
                "refs",
                "--graph",
                "/g.json",
                "--type",
                "App.Leak",
                "--max-depth",
                "4",
                "--max-nodes",
                "50"
            ]
        );
    }

    #[test]
    fn a_missing_engine_is_a_missing_tool() {
        assert_eq!(
            memory_error(EngineError::Missing("where".to_owned())),
            MemoryError::ToolMissing("where".to_owned())
        );
    }
}
