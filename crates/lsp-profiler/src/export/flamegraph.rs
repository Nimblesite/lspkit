//! Flame-graph SVG export via `inferno`. Implements [PROF-EXPORT-FLAMEGRAPH].

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{artifact_path, frame_label, validate_exportable, write_artifact, ExportMeta};
use crate::aggregate::ProfileData;

/// Render `data` as a self-contained interactive SVG.
///
/// # Errors
///
/// Returns a message when the data is not loadable or rendering fails.
pub fn export(
    data: &ProfileData,
    meta: &ExportMeta<'_>,
    output_dir: &Path,
) -> Result<PathBuf, String> {
    validate_exportable(data)?;
    let collapsed = collapsed_stacks(data);
    let mut options = inferno::flamegraph::Options::default();
    options.title = format!("{} profile \u{2014} PID {}", meta.runtime, meta.pid);
    "samples".clone_into(&mut options.count_name);
    options.colors =
        inferno::flamegraph::color::Palette::Basic(inferno::flamegraph::color::BasicPalette::Hot);
    let mut svg: Vec<u8> = Vec::new();
    inferno::flamegraph::from_lines(&mut options, collapsed.lines(), &mut svg)
        .map_err(|err| format!("Flamegraph rendering failed: {err}"))?;
    write_artifact(
        &artifact_path(output_dir, meta.session_id, "flamegraph.svg"),
        &svg,
    )
}

/// `;` separates frames in the collapsed format and inferno has no escaping,
/// so a label containing one would split into phantom frames.
fn fold_safe(text: &str) -> Cow<'_, str> {
    if text.contains(';') {
        Cow::Owned(text.replace(';', ","))
    } else {
        Cow::Borrowed(text)
    }
}

/// Collapsed stack lines: `root;caller;leaf count`, sorted.
pub(crate) fn collapsed_stacks(data: &ProfileData) -> String {
    let mut counts: HashMap<String, u64> = HashMap::new();
    for stack in data.thread_stacks.values().flatten() {
        let line = stack
            .iter()
            .filter_map(|idx| data.frames.get(*idx))
            .map(|frame| fold_safe(&frame_label(frame)).into_owned())
            .collect::<Vec<_>>()
            .join(";");
        *counts.entry(line).or_insert(0) += 1;
    }
    let mut lines: Vec<String> = counts
        .into_iter()
        .map(|(stack, n)| format!("{stack} {n}"))
        .collect();
    lines.sort();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::test_data::{frame, meta, sample_data};
    use super::*;

    #[test]
    fn flamegraph_svg_renders_every_frame() {
        let dir = tempfile::tempdir().expect("create temporary flamegraph directory");
        let path = export(&sample_data(), &meta("fg-1"), dir.path())
            .expect("export representative flamegraph data");
        let svg = std::fs::read_to_string(path).expect("read exported flamegraph SVG");
        assert!(svg.starts_with("<?xml") && svg.contains("<svg"));
        for name in ["&lt;module&gt;", "process", "parse"] {
            assert!(svg.contains(name), "missing frame {name}");
        }
        assert!(svg.contains("Test profile \u{2014} PID 4242"));
    }

    #[test]
    fn collapsed_stacks_aggregate_identical_stacks() {
        let mut data = sample_data();
        let _ = data
            .thread_stacks
            .insert(1, vec![vec![0, 1], vec![0, 1], vec![0]]);
        assert_eq!(
            collapsed_stacks(&data),
            "<module> (main.py:1) 1\n<module> (main.py:1);process (pipeline.py:42) 2"
        );
    }

    // A path containing ';' must not split into phantom frames.
    #[test]
    fn flamegraph_survives_semicolons_in_file_paths() {
        let data = ProfileData {
            frames: vec![
                frame("outer", "/data;backup/main.py", 1),
                frame("inner", "/data;backup/main.py", 9),
            ],
            thread_stacks: HashMap::from([(1, vec![vec![0, 1]])]),
            thread_weights: HashMap::from([(1, vec![0.01])]),
            ..ProfileData::default()
        };
        let dir = tempfile::tempdir().expect("create temporary flamegraph directory");
        let path = export(&data, &meta("semi"), dir.path())
            .expect("export flamegraph data containing semicolons");
        let svg = std::fs::read_to_string(path).expect("read exported flamegraph SVG");
        assert_eq!(
            svg.matches("<title>").count(),
            3,
            "root + 2 frames; more means ';' split the stack"
        );
    }
}
