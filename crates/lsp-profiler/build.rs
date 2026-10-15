//! Regenerates `src/wire_generated.rs` from `docs/models/profiler.td` via
//! `scripts/typediagram/generate.mjs` before rustc runs. The generated file
//! is gitignored, so this script is the only path that produces it on a
//! fresh checkout. Implements [PROF-WIRE-GEN].

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), String> {
    let root = workspace_root()?;
    for input in [
        "docs/models/profiler.td",
        "docs/models/profiler-methods.json",
        "scripts/typediagram/generate.mjs",
        "scripts/typediagram/rust-emit.mjs",
        "scripts/typediagram/ts-emit.mjs",
        "scripts/typediagram/td-docs.mjs",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(input).display());
    }
    let status = Command::new("node")
        .arg(root.join("scripts/typediagram/generate.mjs"))
        .current_dir(&root)
        .status()
        .map_err(|error| format!("typediagram-gen: cannot spawn node ({error}); install Node.js >= 20 and `npm i -g typediagram`"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "typediagram-gen failed ({status}); run `make typediagram-gen` to reproduce"
        ))
    }
}

/// The workspace root: two parents above this crate's manifest directory.
fn workspace_root() -> Result<PathBuf, String> {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| "CARGO_MANIFEST_DIR must be set by cargo".to_owned())?;
    Path::new(&manifest)
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "lsp-profiler must live two levels below the workspace root".to_owned())
}
