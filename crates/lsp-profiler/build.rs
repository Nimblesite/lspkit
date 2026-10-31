//! Regenerates `src/wire_generated.rs` from `docs/models/profiler.td` via
//! `scripts/typediagram/generate.mjs` before rustc runs. The generated file
//! is gitignored, so this script is the only path that produces it on a
//! fresh checkout. Implements [PROF-WIRE-GEN].
//!
//! A published crates.io tarball has no workspace around it, so the generator
//! is absent. The release generates the model before `cargo publish` and the
//! crate's `include` ships it; in that packaged layout this script only checks
//! the shipped model is present. [REL-PACKAGED-BUILD]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The generator entry point, relative to the workspace root.
const GENERATOR: &str = "scripts/typediagram/generate.mjs";

/// The generated wire model, relative to this crate's manifest directory.
const GENERATED: &str = "src/wire_generated.rs";

/// Every generator input; a change to any of them reruns this script.
const INPUTS: [&str; 6] = [
    "docs/models/profiler.td",
    "docs/models/profiler-methods.json",
    GENERATOR,
    "scripts/typediagram/rust-emit.mjs",
    "scripts/typediagram/ts-emit.mjs",
    "scripts/typediagram/td-docs.mjs",
];

fn main() -> Result<(), String> {
    let manifest = manifest_dir()?;
    let root = workspace_root(&manifest)?;
    if root.join(GENERATOR).is_file() {
        generate(&root)
    } else {
        packaged_model(&manifest)
    }
}

/// Workspace build: run the typeDiagram generator.
fn generate(root: &Path) -> Result<(), String> {
    for input in INPUTS {
        println!("cargo:rerun-if-changed={}", root.join(input).display());
    }
    let status = Command::new("node")
        .arg(root.join(GENERATOR))
        .current_dir(root)
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

/// Packaged build: the published tarball MUST carry the generated model.
fn packaged_model(manifest: &Path) -> Result<(), String> {
    let model = manifest.join(GENERATED);
    println!("cargo:rerun-if-changed={}", model.display());
    if model.is_file() {
        Ok(())
    } else {
        Err(format!(
            "lsp-profiler: no workspace generator at {GENERATOR} and no packaged {GENERATED}; \
             build inside the lspkit workspace or from a published crate"
        ))
    }
}

/// This crate's manifest directory, set by cargo for every build script.
fn manifest_dir() -> Result<PathBuf, String> {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "CARGO_MANIFEST_DIR must be set by cargo".to_owned())
}

/// The workspace root: two parents above this crate's manifest directory.
fn workspace_root(manifest: &Path) -> Result<PathBuf, String> {
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "lsp-profiler must live two levels below the workspace root".to_owned())
}
