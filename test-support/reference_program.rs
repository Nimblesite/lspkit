//! Locate exactly the real producer built for this test profile.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};

/// Pipe/process liveness guard; success never depends on elapsed time.
pub const READ_GUARD: Duration = Duration::from_secs(20);

/// Use an explicit producer or this test executable's own Cargo profile.
pub fn reference_binary() -> Result<PathBuf, String> {
    let binary = match std::env::var_os("LSPKIT_REFERENCE_BIN") {
        Some(path) => PathBuf::from(path),
        None => profile_binary()?,
    };
    if binary.is_file() {
        Ok(binary)
    } else {
        Err(format!(
            "reference producer missing at {}; build the real diagnostic-reference example first",
            binary.display()
        ))
    }
}

/// Resolve the producer in this test executable's own profile directory.
///
/// Cargo's build-dir layout moves test executables (`<profile>/deps/` before
/// 1.100, `<profile>/build/<pkg>/<hash>/out/` since), while examples stay in
/// `<profile>/examples/`. The nearest ancestor holding the example is the
/// profile, so either layout finds the producer built with this test.
fn profile_binary() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let name = format!("diagnostic-reference{}", std::env::consts::EXE_SUFFIX);
    let example = Path::new("examples").join(&name);
    executable
        .parent()
        .and_then(|directory| lspkit::path::find_ancestor(directory, &example))
        .ok_or_else(|| format!("no examples/{name} above {}", executable.display()))
}

/// Both test clients use the same actual producer and native stdio setup.
pub fn spawn_reference(args: &[&str]) -> std::io::Result<Child> {
    let binary = reference_binary().map_err(std::io::Error::other)?;
    Command::new(binary)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}
