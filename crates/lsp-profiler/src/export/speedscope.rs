//! Speedscope JSON export. Implements [PROF-EXPORT-SPEEDSCOPE].
//!
//! Frames are already deduplicated at ingest, threads become profiles, and
//! each sample carries its recorded weight in seconds.
//! See <https://www.speedscope.app/file-format-schema.json>.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{artifact_path, sorted_threads, validate_exportable, write_artifact, ExportMeta};
use crate::aggregate::ProfileData;

/// Top-level speedscope document consumed directly by the profile viewer.
#[derive(Serialize)]
struct SpeedscopeFile {
    /// Schema URL that lets speedscope validate the exported document.
    #[serde(rename = "$schema")]
    schema: &'static str,
    /// Frame table shared by every per-thread profile.
    shared: Shared,
    /// Sampled profiles, one for each observed thread.
    profiles: Vec<Profile>,
    /// Human-readable identity of the profiled process and runtime.
    name: String,
    /// Producer name and version shown by profile tooling.
    exporter: String,
    /// Initially selected thread profile; the first profile is the default.
    #[serde(rename = "activeProfileIndex")]
    active_profile_index: usize,
}

/// Deduplicated call-frame data referenced by numeric sample indices.
#[derive(Serialize)]
struct Shared {
    /// Frames in the same order as the aggregator's deduplication table.
    frames: Vec<Frame>,
}

/// Source identity of one frame in the shared table.
#[derive(Serialize)]
struct Frame {
    /// Function or method name displayed in the viewer.
    name: String,
    /// Source path omitted when the runtime could not resolve it.
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    /// One-based source line omitted when no location is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<i64>,
}

/// Speedscope's sampled representation of one thread's activity.
#[derive(Serialize)]
struct Profile {
    /// Format discriminator required for weighted stack samples.
    #[serde(rename = "type")]
    kind: &'static str,
    /// Thread ID and optional runtime-provided thread name.
    name: String,
    /// Unit shared by the timeline bounds and every sample weight.
    unit: &'static str,
    /// Timeline origin, relative to the beginning of this thread's samples.
    #[serde(rename = "startValue")]
    start_value: f64,
    /// Total sampled duration marking the end of the thread timeline.
    #[serde(rename = "endValue")]
    end_value: f64,
    /// Root-first shared-frame indices for each sampled stack.
    samples: Vec<Vec<usize>>,
    /// Recorded duration of each stack, parallel to `samples`.
    weights: Vec<f64>,
}

/// Write `data` as speedscope JSON.
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
    let json = serde_json::to_string(&build(data, meta))
        .map_err(|err| format!("Failed to serialize speedscope JSON: {err}"))?;
    write_artifact(
        &artifact_path(output_dir, meta.session_id, "speedscope.json"),
        json.as_bytes(),
    )
}

/// Convert aggregated stacks and session metadata into speedscope's schema.
fn build(data: &ProfileData, meta: &ExportMeta<'_>) -> SpeedscopeFile {
    let frames = data
        .frames
        .iter()
        .map(|f| Frame {
            name: f.name.clone(),
            file: f.file.clone(),
            line: f.line,
        })
        .collect();
    let profiles = sorted_threads(data)
        .into_iter()
        .map(|tid| thread_profile(data, tid))
        .collect();
    SpeedscopeFile {
        schema: "https://www.speedscope.app/file-format-schema.json",
        shared: Shared { frames },
        profiles,
        name: format!(
            "{} profile \u{2014} PID {} ({:.1}s)",
            meta.runtime, meta.pid, meta.duration_secs
        ),
        exporter: format!("lsp-profiler {}", env!("CARGO_PKG_VERSION")),
        active_profile_index: 0,
    }
}

/// Project one thread's stacks and recorded weights into a sampled profile.
fn thread_profile(data: &ProfileData, tid: i64) -> Profile {
    let name = data.thread_names.get(&tid).map_or_else(
        || format!("Thread {tid}"),
        |n| format!("Thread {tid} ({n})"),
    );
    let weights = data.thread_weights.get(&tid).cloned().unwrap_or_default();
    Profile {
        kind: "sampled",
        name,
        unit: "seconds",
        start_value: 0.0,
        end_value: weights.iter().sum(),
        samples: data.thread_stacks.get(&tid).cloned().unwrap_or_default(),
        weights,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::super::test_data::{meta, sample_data};
    use super::*;
    use crate::aggregate::FrameKey;

    fn read_json(path: &Path) -> Result<Value, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    #[test]
    fn speedscope_json_valid_structure() {
        let dir = tempfile::tempdir().expect("create temporary speedscope directory");
        let path = export(&sample_data(), &meta("ss-1"), dir.path())
            .expect("export representative speedscope data");
        let json = read_json(&path).expect("read exported speedscope JSON");
        assert_eq!(
            json.get("$schema").and_then(Value::as_str),
            Some("https://www.speedscope.app/file-format-schema.json")
        );
        assert_eq!(
            json.pointer("/shared/frames")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(3)
        );
        assert_eq!(
            json.pointer("/profiles/0/samples/0"),
            Some(&serde_json::json!([0, 1, 2]))
        );
        assert_eq!(
            json.pointer("/profiles/0/name").and_then(Value::as_str),
            Some("Thread 1 (MainThread)")
        );
        assert_eq!(
            json.pointer("/profiles/0/endValue").and_then(Value::as_f64),
            Some(0.02)
        );
    }

    // The profile is named after the PROFILED process and its runtime.
    #[test]
    fn profile_name_carries_runtime_and_pid() {
        let dir = tempfile::tempdir().expect("create temporary speedscope directory");
        let path = export(&sample_data(), &meta("ss-2"), dir.path())
            .expect("export named speedscope profile");
        let json = read_json(&path).expect("read exported speedscope JSON");
        assert_eq!(
            json.get("name").and_then(Value::as_str),
            Some("Test profile \u{2014} PID 4242 (2.0s)")
        );
    }

    // Name-only frames omit `file`/`line` rather than writing nulls.
    #[test]
    fn unlocated_frames_omit_location_fields() {
        let dir = tempfile::tempdir().expect("create temporary speedscope directory");
        let mut data = sample_data();
        data.frames = vec![FrameKey {
            name: "Ns.T.M()".to_owned(),
            file: None,
            line: None,
        }];
        let _ = data.thread_stacks.insert(1, vec![vec![0], vec![0]]);
        let path = export(&data, &meta("ss-3"), dir.path())
            .expect("export speedscope data without source locations");
        let json = read_json(&path).expect("read exported speedscope JSON");
        assert_eq!(
            json.pointer("/shared/frames/0"),
            Some(&serde_json::json!({ "name": "Ns.T.M()" }))
        );
    }

    #[test]
    fn export_of_60k_samples_is_fast() {
        let mut data = ProfileData {
            total_samples: 60_000,
            ..ProfileData::default()
        };
        data.frames = (0..200_i64)
            .map(|i| {
                super::super::test_data::frame(
                    &format!("func_{i}"),
                    &format!("m{}.py", i.div_euclid(10)),
                    i % 50 + 1,
                )
            })
            .collect();
        for tid in 1..=4_i64 {
            let stacks: Vec<Vec<usize>> = (0..15_000_usize)
                .map(|s| (0..=s % 5).map(|d| (s + d) % 200).collect())
                .collect();
            let _ = data.thread_weights.insert(tid, vec![0.01; stacks.len()]);
            let _ = data.thread_stacks.insert(tid, stacks);
        }
        let dir = tempfile::tempdir().expect("create temporary speedscope directory");
        let started = std::time::Instant::now();
        let _artifact = export(&data, &meta("bench"), dir.path())
            .expect("export the large speedscope benchmark profile");
        let limit_ms = if cfg!(debug_assertions) { 2_000 } else { 400 };
        assert!(
            started.elapsed().as_millis() < limit_ms,
            "took {:?}",
            started.elapsed()
        );
    }
}
