//! Shared profiler path operations. Implements [PROF-PROCESSES] and [PROF-DOTNET-PROCESSES].

use std::path::{Path, PathBuf};

/// Resolve a relative path against the owning process's directory when known.
#[must_use]
pub fn resolve_against(raw: impl AsRef<Path>, base: Option<&Path>) -> PathBuf {
    let path = raw.as_ref();
    match base {
        Some(base) if path.is_relative() => base.join(path),
        _ => path.to_path_buf(),
    }
}

/// Whether `candidate` is equal to, or nested under, any of `roots`.
#[must_use]
pub fn path_in_roots(candidate: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| candidate.starts_with(root))
}

/// Canonicalize existing paths, retaining the original spelling if unavailable.
#[must_use]
pub fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
