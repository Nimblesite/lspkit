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

/// Resolve the producer in this test executable's exact profile directory.
fn profile_binary() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let profile = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("test executable has no Cargo profile directory")?;
    Ok(profile.join("examples").join(format!(
        "diagnostic-reference{}",
        std::env::consts::EXE_SUFFIX
    )))
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
