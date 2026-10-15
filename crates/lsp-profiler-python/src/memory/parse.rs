//! Parsing of the injected scripts' output. Implements [PROF-PY-MEMORY-INGEST].
//!
//! Each script prints one marker followed by compact JSON. The marker that
//! appears EARLIEST selects the parser — an object `repr` later in the JSON
//! could contain a marker-like substring. Large payloads are handed off
//! through a temp file announced as `__LSPPROF_MEM_FILE__<path>`; the file is
//! an empty reservation until the debuggee's render worker atomically
//! replaces it, so an empty file is polled and any content is complete.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use lsp_profiler::memory::compare::from_changes;
use lsp_profiler::memory::{MemoryError, MemoryPayload};
use lsp_profiler::wire::{
    AllocationSite, GcResult, HeapSnapshot, ObjectList, ObjectSample, RefEdge, RefGraph, RefNode,
    SiteGrowth, TraceFrame, TypeStat, UncollectableObject,
};
use serde::Deserialize;
use serde_json::Value;

/// Prefix shared by every marker; the editor waits for it.
pub const MARKER_PREFIX: &str = "__LSPPROF_";
/// Snapshot payload.
pub const SNAPSHOT_MARKER: &str = "__LSPPROF_MEM__";
/// Diff payload.
pub const DIFF_MARKER: &str = "__LSPPROF_MEM_DIFF__";
/// Retention-graph payload.
pub const REFS_MARKER: &str = "__LSPPROF_MEM_REFS__";
/// Objects-by-type payload.
pub const OBJECTS_MARKER: &str = "__LSPPROF_MEM_OBJECTS__";
/// Garbage-collection payload.
pub const GC_MARKER: &str = "__LSPPROF_MEM_GC__";
/// Acknowledgement.
pub const OK_MARKER: &str = "__LSPPROF_MEM_OK__";
/// File handoff announcement; the path follows on the same line.
pub const FILE_MARKER: &str = "__LSPPROF_MEM_FILE__";

/// How long a handed-off payload file may stay an empty reservation.
const FILE_PAYLOAD_WAIT: Duration = Duration::from_secs(60);
/// Poll interval for the payload file.
const FILE_PAYLOAD_POLL: Duration = Duration::from_millis(100);
/// Longest excerpt quoted back for a marker-less payload.
const EVIDENCE_LIMIT: usize = 400;

/// Resolve a file handoff, then parse. Implements [PROF-PY-MEMORY-COURIER].
///
/// # Errors
///
/// Returns [`MemoryError::Payload`] quoting what arrived when no marker is found.
pub async fn parse_output(output: &str) -> Result<MemoryPayload, MemoryError> {
    let resolved = resolve_file_handoff(output).await;
    parse_payload(&resolved)
}

/// Replace a `__LSPPROF_MEM_FILE__<path>` announcement with the file's content.
async fn resolve_file_handoff(output: &str) -> String {
    let Some(path) = handoff_path(output) else {
        return output.to_owned();
    };
    let deadline = Instant::now() + FILE_PAYLOAD_WAIT;
    loop {
        match tokio::fs::read_to_string(&path).await {
            Ok(contents) if !contents.is_empty() => {
                let _ = tokio::fs::remove_file(&path).await;
                return contents;
            }
            Ok(_) if Instant::now() < deadline => tokio::time::sleep(FILE_PAYLOAD_POLL).await,
            Ok(_) | Err(_) => {
                tracing::warn!("memory payload file never filled or could not be read");
                let _ = tokio::fs::remove_file(&path).await;
                return output.to_owned();
            }
        }
    }
}

/// The path announced by a file handoff, if any.
fn handoff_path(output: &str) -> Option<String> {
    let start = output.find(FILE_MARKER)? + FILE_MARKER.len();
    let path = output.get(start..)?.lines().next()?.trim();
    (!path.is_empty()).then(|| path.to_owned())
}

/// Dispatch on the earliest marker.
///
/// # Errors
///
/// Returns [`MemoryError::Payload`] for missing markers or malformed JSON.
pub fn parse_payload(output: &str) -> Result<MemoryPayload, MemoryError> {
    let markers = [DIFF_MARKER, GC_MARKER, REFS_MARKER, OBJECTS_MARKER, SNAPSHOT_MARKER, OK_MARKER];
    let earliest = markers
        .iter()
        .filter_map(|marker| output.find(marker).map(|at| (at, *marker)))
        .min_by_key(|(at, _)| *at);
    let Some((at, marker)) = earliest else {
        return Err(MemoryError::Payload(format!(
            "no recognized __LSPPROF_MEM*__ marker in script output: {}",
            evidence(output)
        )));
    };
    let json = output.get(at + marker.len()..).and_then(|rest| rest.lines().next()).unwrap_or_default();
    match marker {
        SNAPSHOT_MARKER => decode::<SnapshotJson>(json, "snapshot").map(|s| MemoryPayload::Snapshot(s.into_wire())),
        DIFF_MARKER => parse_diff(json),
        GC_MARKER => decode::<GcJson>(json, "gc").map(|g| MemoryPayload::Gc(g.into_wire())),
        REFS_MARKER => decode::<RefsJson>(json, "reference-graph").map(|r| MemoryPayload::Refs(r.into_wire())),
        OBJECTS_MARKER => decode::<ObjectsJson>(json, "objects-by-type").map(|o| MemoryPayload::Objects(o.into_wire())),
        _ => Ok(MemoryPayload::Ack),
    }
}

fn decode<T: for<'de> Deserialize<'de>>(json: &str, what: &str) -> Result<T, MemoryError> {
    serde_json::from_str(json).map_err(|err| MemoryError::Payload(format!("invalid {what} JSON: {err}")))
}

/// What the courier delivered, folded to one bounded line for the error.
fn evidence(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "<empty>".to_owned();
    }
    let folded: String = trimmed.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    match folded.char_indices().nth(EVIDENCE_LIMIT) {
        Some((end, _)) => format!("{}… ({} bytes total)", folded.get(..end).unwrap_or_default(), output.len()),
        None => folded,
    }
}

/// The diff payload. The first comparison has no baseline yet — the script
/// seeds it — which is a clean empty diff, not an error.
fn parse_diff(json: &str) -> Result<MemoryPayload, MemoryError> {
    let value: DiffJson = decode(json, "diff")?;
    match (value.error, value.leaks) {
        (Some(error), _) if error.contains("no previous snapshot") => Ok(MemoryPayload::Diff(from_changes(Vec::new()))),
        (Some(error), _) => Err(MemoryError::Payload(error)),
        (None, Some(entries)) => Ok(MemoryPayload::Diff(from_changes(entries.into_iter().map(DiffEntry::into_wire).collect()))),
        (None, None) => Err(MemoryError::Payload("invalid diff JSON: missing 'leaks' array".to_owned())),
    }
}

#[derive(Deserialize)]
struct FrameJson {
    #[serde(default)]
    file: String,
    #[serde(default)]
    line: i64,
}

fn frames(frames: Vec<FrameJson>) -> Vec<TraceFrame> {
    frames.into_iter().map(|f| TraceFrame { file: f.file, line: f.line }).collect()
}

/// A located site, unless the script reported no file.
fn location(file: Option<String>, line: i64) -> (Option<String>, Option<i64>) {
    match file {
        Some(file) if !file.is_empty() => (Some(file), Some(line)),
        _ => (None, None),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotJson {
    #[serde(default)]
    current: i64,
    #[serde(default)]
    peak: i64,
    #[serde(default)]
    gc_objects: i64,
    #[serde(default)]
    gc_counts: Vec<i64>,
    #[serde(default)]
    stats: Vec<StatJson>,
}

#[derive(Deserialize)]
struct StatJson {
    file: Option<String>,
    #[serde(default)]
    line: i64,
    #[serde(default)]
    size: i64,
    #[serde(default)]
    count: i64,
    #[serde(default)]
    traceback: Vec<FrameJson>,
}

impl SnapshotJson {
    fn into_wire(self) -> HeapSnapshot {
        let sites = self
            .stats
            .into_iter()
            .map(|stat| {
                let (file, line) = location(stat.file, stat.line);
                AllocationSite { file, line, type_name: None, size_bytes: stat.size, count: stat.count, traceback: frames(stat.traceback) }
            })
            .collect();
        HeapSnapshot {
            snapshot_id: String::new(),
            current_bytes: self.current,
            peak_bytes: self.peak,
            object_count: self.gc_objects,
            gc_generations: self.gc_counts,
            sites,
        }
    }
}

#[derive(Deserialize)]
struct DiffJson {
    error: Option<String>,
    leaks: Option<Vec<DiffEntry>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiffEntry {
    file: Option<String>,
    #[serde(default)]
    line: i64,
    #[serde(default)]
    size_diff: i64,
    #[serde(default)]
    count_diff: i64,
    #[serde(default)]
    size: i64,
    #[serde(default)]
    count: i64,
    #[serde(default)]
    traceback: Vec<FrameJson>,
}

impl DiffEntry {
    fn into_wire(self) -> SiteGrowth {
        let (file, line) = location(self.file, self.line);
        SiteGrowth { file, line, type_name: None, size_diff: self.size_diff, count_diff: self.count_diff, size_bytes: self.size, count: self.count, traceback: frames(self.traceback) }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GcJson {
    #[serde(default)]
    collected: i64,
    #[serde(default)]
    uncollectable: i64,
    #[serde(default)]
    memory_freed: i64,
    #[serde(default)]
    uncollectable_objects: Vec<ObjectJson>,
}

impl GcJson {
    fn into_wire(self) -> GcResult {
        GcResult {
            collected: self.collected,
            uncollectable_count: self.uncollectable,
            freed_bytes: self.memory_freed,
            uncollectable: self
                .uncollectable_objects
                .into_iter()
                .map(|o| UncollectableObject {
                    id: id_text(&o.id),
                    type_name: o.type_name.unwrap_or_else(|| "<unknown>".to_owned()),
                    label: o.repr.unwrap_or_default(),
                    size_bytes: o.size,
                    reason: o.reason.unwrap_or_else(|| "Uncollectable cycle".to_owned()),
                })
                .collect(),
        }
    }
}

/// A Python object as the scripts report it.
#[derive(Deserialize)]
struct ObjectJson {
    #[serde(default)]
    id: Value,
    #[serde(rename = "type")]
    type_name: Option<String>,
    #[serde(default)]
    size: i64,
    repr: Option<String>,
    refcount: Option<i64>,
    reason: Option<String>,
}

/// Object identities arrive as integers (`id(obj)`); the wire carries strings.
fn id_text(id: &Value) -> String {
    match id {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[derive(Deserialize)]
struct RefsJson {
    #[serde(default)]
    nodes: Vec<RefNodeJson>,
    #[serde(default)]
    edges: Vec<RefEdgeJson>,
    #[serde(default)]
    cycles: Vec<Vec<Value>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefNodeJson {
    #[serde(default)]
    id: Value,
    #[serde(rename = "type", default)]
    type_name: String,
    #[serde(default)]
    size: i64,
    #[serde(default)]
    repr: String,
    #[serde(default)]
    depth: i64,
    #[serde(default)]
    is_target: bool,
}

#[derive(Deserialize)]
struct RefEdgeJson {
    #[serde(default)]
    from: Value,
    #[serde(default)]
    to: Value,
    label: Option<String>,
}

impl RefsJson {
    fn into_wire(self) -> RefGraph {
        RefGraph {
            nodes: self
                .nodes
                .into_iter()
                .map(|n| RefNode {
                    id: id_text(&n.id),
                    type_name: n.type_name,
                    label: n.repr,
                    size_bytes: n.size,
                    retained_bytes: None,
                    depth: n.depth,
                    is_target: n.is_target,
                    is_root: false,
                    root_kind: None,
                })
                .collect(),
            edges: self.edges.into_iter().map(|e| RefEdge { from: id_text(&e.from), to: id_text(&e.to), label: e.label }).collect(),
            cycles: self.cycles.into_iter().map(|cycle| cycle.iter().map(id_text).collect()).collect(),
            truncated: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObjectsJson {
    #[serde(default)]
    objects: Vec<ObjectJson>,
    #[serde(default)]
    total_count: i64,
    #[serde(default)]
    total_size: i64,
    #[serde(default)]
    type_summary: BTreeMap<String, TypeTotals>,
}

#[derive(Deserialize)]
struct TypeTotals {
    #[serde(default)]
    count: i64,
    #[serde(default)]
    size: i64,
}

/// Types listed in an objects-by-type summary.
const TYPE_SUMMARY_LIMIT: usize = 50;

impl ObjectsJson {
    fn into_wire(self) -> ObjectList {
        let type_name = self.objects.first().and_then(|o| o.type_name.clone()).unwrap_or_default();
        let mut summary: Vec<TypeStat> = self
            .type_summary
            .into_iter()
            .map(|(name, totals)| TypeStat { type_name: name, count: totals.count, size_bytes: totals.size })
            .collect();
        summary.sort_by_key(|stat| std::cmp::Reverse(stat.size_bytes));
        summary.truncate(TYPE_SUMMARY_LIMIT);
        ObjectList {
            type_name,
            total_count: self.total_count,
            total_bytes: self.total_size,
            objects: self
                .objects
                .into_iter()
                .map(|o| ObjectSample {
                    id: id_text(&o.id),
                    type_name: o.type_name.unwrap_or_default(),
                    size_bytes: o.size,
                    refcount: o.refcount,
                    label: o.repr.unwrap_or_default(),
                })
                .collect(),
            type_summary: summary,
        }
    }
}

/// Read a final at-exit snapshot file, deleting it only once a complete,
/// marker-bearing payload was read. Implements [PROF-PY-MEMORY-FINAL].
pub async fn read_final_snapshot(path: &Path, wait: Duration) -> Option<String> {
    let deadline = Instant::now() + wait;
    loop {
        if let Ok(contents) = tokio::fs::read_to_string(path).await {
            if contents.contains(SNAPSHOT_MARKER) {
                let _ = tokio::fs::remove_file(path).await;
                return Some(contents);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(FILE_PAYLOAD_POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(output: &str) -> Result<HeapSnapshot, String> {
        match parse_payload(output).map_err(|e| e.to_string())? {
            MemoryPayload::Snapshot(snapshot) => Ok(snapshot),
            other => Err(format!("expected a snapshot, got {other:?}")),
        }
    }

    #[test]
    fn parses_a_snapshot_into_located_sites() -> Result<(), String> {
        let output = r#"debug noise
__LSPPROF_MEM__{"current": 45678912, "peak": 50000000, "gcObjects": 14523, "gcCounts": [712, 45, 3], "stats": [{"file": "/src/app.py", "line": 42, "size": 24567890, "count": 15234, "traceback": [{"file": "/src/main.py", "line": 3}, {"file": "/src/app.py", "line": 42}]}]}
trailing"#;
        let parsed = snapshot(output)?;
        assert_eq!((parsed.current_bytes, parsed.peak_bytes, parsed.object_count), (45_678_912, 50_000_000, 14_523));
        assert_eq!(parsed.gc_generations, vec![712, 45, 3]);
        let site = parsed.sites.first().ok_or("one site")?;
        assert_eq!((site.file.as_deref(), site.line, site.size_bytes, site.count), (Some("/src/app.py"), Some(42), 24_567_890, 15_234));
        assert_eq!(site.traceback.first().map(|f| (f.file.as_str(), f.line)), Some(("/src/main.py", 3)));
        Ok(())
    }

    #[test]
    fn diff_splits_growth_and_frees_and_first_diff_is_empty() -> Result<(), String> {
        let output = r#"__LSPPROF_MEM_DIFF__{"leaks": [{"file": "/src/cache.py", "line": 34, "sizeDiff": 2000, "countDiff": 20, "size": 9000, "count": 90, "traceback": []}, {"file": "/src/tmp.py", "line": 8, "sizeDiff": -500, "countDiff": -5, "size": 0, "count": 0, "traceback": []}]}"#;
        let MemoryPayload::Diff(diff) = parse_payload(output).map_err(|e| e.to_string())? else {
            return Err("expected a diff".to_owned());
        };
        assert_eq!((diff.total_growth, diff.total_freed, diff.net_growth), (2000, 500, 1500));
        assert_eq!(diff.grown.first().map(|g| (g.file.as_deref(), g.line)), Some((Some("/src/cache.py"), Some(34))));
        let first = parse_payload(r#"__LSPPROF_MEM_DIFF__{"error": "no previous snapshot"}"#).map_err(|e| e.to_string())?;
        assert_eq!(first, MemoryPayload::Diff(from_changes(Vec::new())));
        assert!(parse_payload(r#"__LSPPROF_MEM_DIFF__{"error": "tracemalloc is not tracing"}"#).is_err_and(|e| e.to_string() == "tracemalloc is not tracing"));
    }

    #[test]
    fn parses_gc_refs_objects_and_ack() -> Result<(), String> {
        let gc = parse_payload(r#"__LSPPROF_MEM_GC__{"collected": 12, "uncollectable": 1, "memoryFreed": 4096, "uncollectableObjects": [{"id": 140, "type": "Node", "size": 64, "repr": "<Node>", "reason": "has __del__"}]}"#).map_err(|e| e.to_string())?;
        let MemoryPayload::Gc(gc) = gc else { return Err("gc".to_owned()) };
        assert_eq!((gc.collected, gc.uncollectable_count, gc.freed_bytes), (12, 1, 4096));
        assert_eq!(gc.uncollectable.first().map(|o| (o.id.as_str(), o.type_name.as_str(), o.reason.as_str())), Some(("140", "Node", "has __del__")));
        let refs = parse_payload(r#"__LSPPROF_MEM_REFS__{"nodes": [{"id": 1, "type": "list", "size": 88, "repr": "[...]", "depth": 0, "isTarget": true}], "edges": [{"from": 2, "to": 1, "label": "items"}], "cycles": [[1, 2, 1]]}"#).map_err(|e| e.to_string())?;
        let MemoryPayload::Refs(graph) = refs else { return Err("refs".to_owned()) };
        assert_eq!(graph.nodes.first().map(|n| (n.id.as_str(), n.is_target, n.label.as_str())), Some(("1", true, "[...]")));
        assert_eq!(graph.edges.first().map(|e| (e.from.as_str(), e.to.as_str(), e.label.as_deref())), Some(("2", "1", Some("items"))));
        assert_eq!(graph.cycles, vec![vec!["1".to_owned(), "2".to_owned(), "1".to_owned()]]);
        let objects = parse_payload(r#"__LSPPROF_MEM_OBJECTS__{"objects": [{"id": 9, "type": "Row", "size": 120, "refcount": 3, "repr": "Row(1)"}], "totalCount": 50, "totalSize": 6000, "typeSummary": {"Row": {"count": 50, "size": 6000}, "str": {"count": 9, "size": 900}}}"#).map_err(|e| e.to_string())?;
        let MemoryPayload::Objects(list) = objects else { return Err("objects".to_owned()) };
        assert_eq!((list.type_name.as_str(), list.total_count, list.total_bytes), ("Row", 50, 6000));
        assert_eq!(list.type_summary.iter().map(|t| t.type_name.as_str()).collect::<Vec<_>>(), vec!["Row", "str"]);
        assert_eq!(parse_payload("__LSPPROF_MEM_OK__").map_err(|e| e.to_string())?, MemoryPayload::Ack);
        Ok(())
    }

    // The EARLIEST marker wins, so a repr containing a marker-like string
    // later in the payload never changes the dispatch.
    #[test]
    fn earliest_marker_selects_the_parser() -> Result<(), String> {
        let output = r#"__LSPPROF_MEM__{"current": 1, "stats": [{"file": "/a.py", "line": 1, "size": 1, "count": 1, "repr": "__LSPPROF_MEM_GC__"}]}"#;
        assert_eq!(snapshot(output)?.current_bytes, 1);
        Ok(())
    }

    #[test]
    fn markerless_output_is_rejected_with_evidence() {
        let error = parse_payload("Traceback (most recent call last):\n  boom").map(|_| ()).map_err(|e| e.to_string());
        assert_eq!(error, Err("no recognized __LSPPROF_MEM*__ marker in script output: Traceback (most recent call last):   boom".to_owned()));
        assert!(parse_payload("   ").is_err_and(|e| e.to_string().ends_with("<empty>")));
    }

    // [PROF-PY-MEMORY-COURIER] An empty reservation is polled until the worker
    // fills it; the file is consumed and deleted.
    #[tokio::test]
    async fn file_handoff_waits_for_the_render_worker() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let file = dir.path().join("payload.txt");
        std::fs::write(&file, "").map_err(|e| e.to_string())?;
        let writer_path = file.clone();
        let writer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            tokio::fs::write(writer_path, r#"__LSPPROF_MEM__{"current": 77}"#).await
        });
        let announced = format!("{FILE_MARKER}{}\n", file.display());
        let payload = parse_output(&announced).await.map_err(|e| e.to_string())?;
        writer.await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
        assert!(matches!(payload, MemoryPayload::Snapshot(ref s) if s.current_bytes == 77), "{payload:?}");
        assert!(!file.exists(), "the handed-off file is deleted once read");
        Ok(())
    }

    #[tokio::test]
    async fn final_snapshot_is_read_only_when_complete() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let file = dir.path().join("final.memfinal");
        assert_eq!(read_final_snapshot(&file, Duration::from_millis(150)).await, None, "missing file yields nothing");
        std::fs::write(&file, "partial").map_err(|e| e.to_string())?;
        assert_eq!(read_final_snapshot(&file, Duration::from_millis(150)).await, None);
        assert!(file.exists(), "a marker-less read is never destructive");
        std::fs::write(&file, "__LSPPROF_MEM__{}").map_err(|e| e.to_string())?;
        assert_eq!(read_final_snapshot(&file, Duration::from_millis(150)).await.as_deref(), Some("__LSPPROF_MEM__{}"));
        assert!(!file.exists());
        Ok(())
    }
}
