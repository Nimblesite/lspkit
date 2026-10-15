//! Profile exports. Implements [PROF-EXPORT].
//!
//! Every stop or snapshot writes three artifacts from the same aggregated
//! data: speedscope JSON, a self-contained flame-graph SVG, and a V8
//! `.cpuprofile` for the editor's built-in viewer. Each export refuses data a
//! viewer could not load ([PROF-EXPORT-VALIDATE]) and reports why instead of
//! failing silently.

pub mod cpuprofile;
pub mod flamegraph;
pub mod speedscope;

use std::path::{Path, PathBuf};

use crate::aggregate::{FrameKey, ProfileData};
use crate::wire::ProfileArtifacts;

/// Identity of the profile being exported.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportMeta<'a> {
    /// Session identifier, used in file names.
    pub session_id: &'a str,
    /// Profiled process.
    pub pid: u32,
    /// Runtime display name, used in titles.
    pub runtime: &'a str,
    /// Seconds recorded.
    pub duration_secs: f64,
    /// Samples per second.
    pub sample_rate: u64,
}

/// Write all artifacts into `output_dir`, collecting every failure.
#[must_use]
pub fn export_artifacts(
    data: &ProfileData,
    meta: &ExportMeta<'_>,
    output_dir: &Path,
) -> ProfileArtifacts {
    let mut errors: Vec<String> = Vec::new();
    let mut keep = |result: Result<PathBuf, String>| match result {
        Ok(path) => Some(path.display().to_string()),
        Err(error) => {
            tracing::error!(%error, "profile export failed");
            if !errors.contains(&error) {
                errors.push(error);
            }
            None
        }
    };
    let speedscope_path = keep(speedscope::export(data, meta, output_dir));
    let flamegraph_path = keep(flamegraph::export(data, meta, output_dir));
    let cpuprofile_path = keep(cpuprofile::export(data, meta, output_dir));
    ProfileArtifacts {
        speedscope_path,
        flamegraph_path,
        cpuprofile_path,
        export_error: (!errors.is_empty()).then(|| errors.join("; ")),
    }
}

/// Path of one artifact: `lsp-profiler-<session>.<extension>`.
#[must_use]
pub fn artifact_path(output_dir: &Path, session_id: &str, extension: &str) -> PathBuf {
    output_dir.join(format!("lsp-profiler-{session_id}.{extension}"))
}

/// Write `contents` to `path`, naming the path in any error.
///
/// # Errors
///
/// Returns a message when the write fails.
pub fn write_artifact(path: &Path, contents: &[u8]) -> Result<PathBuf, String> {
    std::fs::write(path, contents)
        .map(|()| path.to_path_buf())
        .map_err(|err| format!("Failed to write {}: {err}", path.display()))
}

/// Human-readable label of a frame: `name (file:line)` when located.
#[must_use]
pub fn frame_label(frame: &FrameKey) -> String {
    match (&frame.file, frame.line) {
        (Some(file), Some(line)) => format!("{} ({file}:{line})", frame.name),
        (Some(file), None) => format!("{} ({file})", frame.name),
        _ => frame.name.clone(),
    }
}

/// Refuse data an external viewer cannot load. Implements [PROF-EXPORT-VALIDATE].
///
/// Viewers index `frames` by every sample entry, walk parallel sample/weight
/// arrays, and need at least one sample; a file violating any of those loads
/// as an opaque error, so it is never written.
///
/// # Errors
///
/// Returns a description of the first violated invariant.
pub fn validate_exportable(data: &ProfileData) -> Result<(), String> {
    let sample_count: usize = data.thread_stacks.values().map(Vec::len).sum();
    if sample_count == 0 {
        return Err("no samples were collected — nothing to export".to_owned());
    }
    data.thread_stacks
        .iter()
        .try_for_each(|(tid, stacks)| validate_thread(data, *tid, stacks))
}

/// Validate one thread's stacks and weights.
fn validate_thread(data: &ProfileData, tid: i64, stacks: &[Vec<usize>]) -> Result<(), String> {
    let weights = data.thread_weights.get(&tid).map_or(&[][..], Vec::as_slice);
    if stacks.len() != weights.len() {
        return Err(format!(
            "thread {tid}: {} samples but {} weights — counts must match",
            stacks.len(),
            weights.len()
        ));
    }
    if let Some(bad) = weights.iter().find(|w| !w.is_finite() || **w < 0.0) {
        return Err(format!(
            "thread {tid}: weights must be finite and non-negative, got {bad}"
        ));
    }
    let frame_count = data.frames.len();
    match stacks.iter().flatten().find(|idx| **idx >= frame_count) {
        Some(idx) => Err(format!(
            "thread {tid}: frame index {idx} out of bounds ({frame_count} frames)"
        )),
        None => Ok(()),
    }
}

/// Thread IDs in ascending order, for deterministic exports.
#[must_use]
pub fn sorted_threads(data: &ProfileData) -> Vec<i64> {
    let mut ids: Vec<i64> = data.thread_stacks.keys().copied().collect();
    ids.sort_unstable();
    ids
}

#[cfg(test)]
pub(crate) mod test_data {
    use std::collections::HashMap;

    use crate::aggregate::{FrameKey, ProfileData};

    use super::ExportMeta;

    /// A located frame.
    pub fn frame(name: &str, file: &str, line: i64) -> FrameKey {
        FrameKey {
            name: name.to_owned(),
            file: Some(file.to_owned()),
            line: Some(line),
        }
    }

    /// Two samples on one thread over three frames.
    pub fn sample_data() -> ProfileData {
        ProfileData {
            frames: vec![
                frame("<module>", "main.py", 1),
                frame("process", "pipeline.py", 42),
                frame("parse", "parser.py", 15),
            ],
            thread_stacks: HashMap::from([(1, vec![vec![0, 1, 2], vec![0, 1]])]),
            thread_weights: HashMap::from([(1, vec![0.01, 0.01])]),
            thread_names: HashMap::from([(1, "MainThread".to_owned())]),
            thread_samples: HashMap::from([(1, 2)]),
            total_samples: 2,
            ..ProfileData::default()
        }
    }

    /// Export metadata for tests.
    pub fn meta(session_id: &str) -> ExportMeta<'_> {
        ExportMeta {
            session_id,
            pid: 4242,
            runtime: "Test",
            duration_secs: 2.0,
            sample_rate: 100,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_data::{meta, sample_data};
    use super::*;

    #[test]
    fn exports_all_three_artifacts() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let artifacts = export_artifacts(&sample_data(), &meta("all-three"), dir.path());
        assert_eq!(artifacts.export_error, None);
        for path in [
            &artifacts.speedscope_path,
            &artifacts.flamegraph_path,
            &artifacts.cpuprofile_path,
        ] {
            let path = path.as_deref().ok_or("every artifact must be written")?;
            assert!(Path::new(path).exists(), "{path} must exist");
            assert!(
                path.contains("lsp-profiler-all-three."),
                "{path} must be named after the session"
            );
        }
        Ok(())
    }

    // [PROF-EXPORT-VALIDATE] An empty profile is refused by every exporter and
    // the reason is reported once, never silently.
    #[test]
    fn empty_profile_reports_a_single_reason() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let artifacts = export_artifacts(&ProfileData::default(), &meta("empty"), dir.path());
        assert_eq!(artifacts.speedscope_path, None);
        assert_eq!(artifacts.flamegraph_path, None);
        assert_eq!(artifacts.cpuprofile_path, None);
        assert_eq!(
            artifacts.export_error.as_deref(),
            Some("no samples were collected — nothing to export")
        );
        Ok(())
    }

    #[test]
    fn validation_rejects_mismatched_weights_and_bad_indexes() {
        let mut data = sample_data();
        let _ = data.thread_weights.insert(1, vec![0.01]);
        assert!(validate_exportable(&data).is_err_and(|e| e.contains("2 samples but 1 weights")));
        let mut data = sample_data();
        let _ = data.thread_weights.insert(1, vec![0.01, f64::NAN]);
        assert!(validate_exportable(&data).is_err_and(|e| e.contains("finite")));
        let mut data = sample_data();
        let _ = data.thread_stacks.insert(1, vec![vec![0, 9], vec![0]]);
        assert!(
            validate_exportable(&data).is_err_and(|e| e.contains("frame index 9 out of bounds"))
        );
    }

    #[test]
    fn frame_label_omits_missing_location() {
        assert_eq!(frame_label(&test_data::frame("f", "a.py", 3)), "f (a.py:3)");
        let bare = FrameKey {
            name: "Ns.T.M()".to_owned(),
            file: None,
            line: None,
        };
        assert_eq!(frame_label(&bare), "Ns.T.M()");
    }
}
