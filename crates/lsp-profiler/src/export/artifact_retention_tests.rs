//! [MEM-060-ARTIFACTS]: a server that profiles for days writes a heap profile
//! per snapshot and three CPU artifacts per recording. The output directory
//! must keep only the newest of each kind.

use std::path::Path;

use super::{artifact_path, write_artifact, MAX_ARTIFACTS_PER_KIND};

/// Artifacts written: well past the bound.
const WRITTEN: usize = MAX_ARTIFACTS_PER_KIND + 16;
/// An artifact kind.
const KIND: &str = "heapprofile";
/// A file the profiler did not write.
const FOREIGN: &str = "notes.heapprofile";

/// Profiler artifacts of `kind` in `dir`.
fn artifacts(dir: &Path, kind: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        if path.extension().is_some_and(|ext| ext == kind) {
            names.extend(name);
        }
    }
    Ok(names)
}

#[test]
fn the_output_directory_keeps_only_the_newest_artifacts_of_each_kind() -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    std::fs::write(dir.path().join(FOREIGN), b"user file").map_err(|error| error.to_string())?;
    for index in 0..WRITTEN {
        let _written = write_artifact(
            &artifact_path(dir.path(), &format!("snap-{index:04}"), KIND),
            b"{}",
        )?;
    }
    let kept = artifacts(dir.path(), KIND)?;
    if kept.len() > MAX_ARTIFACTS_PER_KIND + 1 {
        return Err(format!(
            "{} {KIND} files kept for a bound of {MAX_ARTIFACTS_PER_KIND}",
            kept.len()
        ));
    }
    let newest = format!("lsp-profiler-snap-{:04}.{KIND}", WRITTEN - 1);
    if !kept.contains(&newest) {
        return Err(format!("the newest artifact {newest} was removed"));
    }
    if !kept.iter().any(|name| name == FOREIGN) {
        return Err("a file the profiler did not write was removed".to_owned());
    }
    Ok(())
}
