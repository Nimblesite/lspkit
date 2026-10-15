//! V8 `.cpuprofile` export. Implements [PROF-EXPORT-CPUPROFILE].
//!
//! The Chrome `DevTools` `Profiler.Profile` schema, so VS Code's built-in
//! profile viewer renders it natively (flame chart, bottom-up and left-heavy
//! tables). All threads merge into one call tree of `nodes` plus the
//! `samples` / `timeDeltas` arrays the viewer needs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{artifact_path, sorted_threads, validate_exportable, write_artifact, ExportMeta};
use crate::aggregate::{FrameKey, ProfileData};

/// One call-tree node (`id` = index + 1).
struct CpuNode {
    /// Index into `ProfileData::frames`; `None` for the synthetic root.
    frame: Option<usize>,
    /// Samples whose leaf is this node.
    hit_count: u64,
    /// Child node indices.
    children: Vec<usize>,
}

/// Incremental call-tree builder.
struct CallTree {
    nodes: Vec<CpuNode>,
    child_of: HashMap<(usize, usize), usize>,
}

impl CallTree {
    fn new() -> Self {
        Self {
            nodes: vec![CpuNode {
                frame: None,
                hit_count: 0,
                children: Vec::new(),
            }],
            child_of: HashMap::new(),
        }
    }

    /// Walk a root-first stack, creating nodes; returns the leaf's node index.
    fn insert(&mut self, stack: &[usize]) -> usize {
        let leaf = stack
            .iter()
            .fold(0, |parent, frame| self.child(parent, *frame));
        if let Some(node) = self.nodes.get_mut(leaf) {
            node.hit_count += 1;
        }
        leaf
    }

    fn child(&mut self, parent: usize, frame: usize) -> usize {
        if let Some(existing) = self.child_of.get(&(parent, frame)) {
            return *existing;
        }
        let index = self.nodes.len();
        self.nodes.push(CpuNode {
            frame: Some(frame),
            hit_count: 0,
            children: Vec::new(),
        });
        if let Some(parent_node) = self.nodes.get_mut(parent) {
            parent_node.children.push(index);
        }
        let _ = self.child_of.insert((parent, frame), index);
        index
    }
}

/// Seconds to whole microseconds, clamped to `[0, 1e15]`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "f64 -> i64 has no TryFrom; the value is rounded and clamped into i64 range first"
)]
fn secs_to_micros(secs: f64) -> i64 {
    (secs * 1_000_000.0).round().clamp(0.0, 1.0e15) as i64
}

/// Build a V8 `Profiler.Profile` value from aggregated data.
#[must_use]
pub fn build(data: &ProfileData) -> Value {
    let mut tree = CallTree::new();
    let mut samples: Vec<usize> = Vec::new();
    let mut time_deltas: Vec<i64> = Vec::new();
    for tid in sorted_threads(data) {
        let stacks = data.thread_stacks.get(&tid).map_or(&[][..], Vec::as_slice);
        let weights = data.thread_weights.get(&tid).map_or(&[][..], Vec::as_slice);
        for (stack, weight) in stacks.iter().zip(weights) {
            samples.push(tree.insert(stack) + 1);
            time_deltas.push(secs_to_micros(*weight));
        }
    }
    let nodes: Vec<Value> = tree
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| node_json(index, node, &data.frames))
        .collect();
    json!({
        "nodes": nodes,
        "startTime": 0,
        "endTime": time_deltas.iter().sum::<i64>(),
        "samples": samples,
        "timeDeltas": time_deltas,
    })
}

/// Write `data` as a `.cpuprofile`.
///
/// A zero-sample profile is refused: VS Code's viewer reads
/// `samples[timeDeltas.length - 1]` and throws on an empty array.
///
/// # Errors
///
/// Returns a message when the data is not loadable or the write fails.
pub fn export(
    data: &ProfileData,
    meta: &ExportMeta<'_>,
    output_dir: &Path,
) -> Result<PathBuf, String> {
    validate_exportable(data)?;
    let json = serde_json::to_string(&build(data))
        .map_err(|err| format!("Failed to serialize cpuprofile: {err}"))?;
    write_artifact(
        &artifact_path(output_dir, meta.session_id, "cpuprofile"),
        json.as_bytes(),
    )
}

fn node_json(index: usize, node: &CpuNode, frames: &[FrameKey]) -> Value {
    json!({
        "id": index + 1,
        "callFrame": call_frame(node.frame.and_then(|i| frames.get(i))),
        "hitCount": node.hit_count,
        "children": node.children.iter().map(|c| c + 1).collect::<Vec<_>>(),
    })
}

/// A `Runtime.CallFrame`; V8 line numbers are 0-based.
fn call_frame(frame: Option<&FrameKey>) -> Value {
    match frame {
        None => {
            json!({ "functionName": "(root)", "scriptId": "0", "url": "", "lineNumber": -1, "columnNumber": -1 })
        }
        Some(frame) => json!({
            "functionName": frame.name,
            "scriptId": "0",
            "url": frame.file.clone().unwrap_or_default(),
            "lineNumber": frame.line.map_or(-1, |line| (line - 1).max(0)),
            "columnNumber": 0,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::test_data::{frame, meta};
    use super::*;

    fn single_frame_data(weights: Vec<f64>) -> ProfileData {
        ProfileData {
            frames: vec![frame("hot_function", "/tmp/app.py", 19)],
            thread_stacks: HashMap::from([(1, vec![vec![0]; weights.len()])]),
            thread_weights: HashMap::from([(1, weights)]),
            ..ProfileData::default()
        }
    }

    // [PROF-EXPORT-CPUPROFILE] nodes/samples/timeDeltas with 0-based lines.
    #[test]
    fn cpuprofile_matches_v8_schema() {
        let profile = build(&single_frame_data(vec![0.01, 0.01]));
        assert_eq!(
            profile
                .pointer("/nodes")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            profile.pointer("/nodes/1/hitCount").and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(
            profile
                .pointer("/nodes/1/callFrame/functionName")
                .and_then(Value::as_str),
            Some("hot_function")
        );
        assert_eq!(
            profile
                .pointer("/nodes/1/callFrame/url")
                .and_then(Value::as_str),
            Some("/tmp/app.py")
        );
        assert_eq!(
            profile
                .pointer("/nodes/1/callFrame/lineNumber")
                .and_then(Value::as_i64),
            Some(18)
        );
        assert_eq!(
            profile
                .pointer("/nodes/0/children/0")
                .and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(profile.get("samples"), Some(&json!([2, 2])));
        assert_eq!(profile.get("timeDeltas"), Some(&json!([10_000, 10_000])));
        assert_eq!(profile.get("endTime").and_then(Value::as_i64), Some(20_000));
    }

    // Recorded weights (not a fixed rate) drive the time deltas, so tools that
    // report variable sample durations keep their timing.
    #[test]
    fn time_deltas_follow_recorded_weights() {
        let profile = build(&single_frame_data(vec![0.001, 0.25]));
        assert_eq!(profile.get("timeDeltas"), Some(&json!([1_000, 250_000])));
    }

    #[test]
    fn unlocated_frames_have_no_line() {
        let mut data = single_frame_data(vec![0.01]);
        data.frames = vec![FrameKey {
            name: "Ns.T.M()".to_owned(),
            file: None,
            line: None,
        }];
        let profile = build(&data);
        assert_eq!(
            profile
                .pointer("/nodes/1/callFrame/lineNumber")
                .and_then(Value::as_i64),
            Some(-1)
        );
        assert_eq!(
            profile
                .pointer("/nodes/1/callFrame/url")
                .and_then(Value::as_str),
            Some("")
        );
    }

    #[test]
    fn export_refuses_a_zero_sample_profile() {
        let dir = std::env::temp_dir();
        assert!(export(&ProfileData::default(), &meta("empty"), &dir).is_err());
    }

    #[test]
    fn export_writes_a_loadable_file() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let path = export(&single_frame_data(vec![0.01]), &meta("cp"), dir.path())?;
        assert!(path.extension().is_some_and(|ext| ext == "cpuprofile"));
        let parsed: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        assert_eq!(
            parsed
                .pointer("/nodes")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
        Ok(())
    }
}
