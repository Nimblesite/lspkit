//! Shared native source owner for compiler and reference-server tests. [LSP-PULL-CONCURRENCY]
use std::io::{self, BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

/// `CREATE_NO_WINDOW`: the owner never opens a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Run only the owned oplock fixture, invisibly and without persistent policy changes.
pub fn spawn(script: &Path, source: &Path) -> io::Result<Child> {
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .arg(script)
        .arg("-Source")
        .arg(source)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

/// Parse fixture state, never infer readiness from elapsed time.
pub fn state(output: &mut BufReader<ChildStdout>, expected: &str) -> io::Result<()> {
    let mut line = String::new();
    let _length = output.read_line(&mut line)?;
    let value: serde_json::Value = serde_json::from_str(&line).map_err(io::Error::other)?;
    if value.get("state").and_then(serde_json::Value::as_str) != Some(expected) {
        return Err(io::Error::other("unexpected native source state"));
    }
    Ok(())
}
