//! Locating and running the engine. Implements [PROF-DOTNET-ENGINE] and
//! [PROF-DOTNET-ENGINE-LOCATE].
//!
//! The engine is one executable with one command per operation. A one-shot
//! command runs to completion and yields its single result message; the CPU
//! command stays attached until a line arrives on its stdin.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tracing::{debug, warn};

use crate::protocol::EngineMessage;

/// Environment variable naming the engine executable.
pub const ENGINE_ENV: &str = "LSP_PROFILER_DOTNET_ENGINE";
/// The engine executable's file stem.
pub const ENGINE_STEM: &str = "lsp-profiler-dotnet-engine";

/// Why the engine could not run or answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    /// No engine executable was found; the message says where it was sought.
    #[error("{0}")]
    Missing(String),
    /// The engine could not be spawned.
    #[error("cannot start the .NET profiling engine: {0}")]
    Spawn(String),
    /// The engine reported a failure.
    #[error("{message}")]
    Failed {
        /// Classified cause.
        kind: crate::protocol::ErrorKind,
        /// Engine detail.
        message: String,
    },
    /// The engine ended without the expected message.
    #[error("the .NET profiling engine ended without answering{0}")]
    NoAnswer(String),
    /// A line could not be read or decoded.
    #[error("cannot read the .NET profiling engine: {0}")]
    Io(String),
}

/// Handle on the engine executable.
#[derive(Debug, Clone)]
pub struct Engine {
    /// Overrides discovery with an explicit executable when configured.
    configured: Option<PathBuf>,
}

impl Engine {
    /// An engine at `configured`, or discovered when `None`.
    #[must_use]
    pub fn new(configured: Option<PathBuf>) -> Self {
        Self { configured }
    }

    /// The executable: configured path, then `LSP_PROFILER_DOTNET_ENGINE`, then beside the host.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Missing`] when none of them exists.
    pub fn locate(&self) -> Result<PathBuf, EngineError> {
        let beside_host = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(executable_name())));
        let candidates = [
            self.configured.clone(),
            std::env::var_os(ENGINE_ENV).map(PathBuf::from),
            beside_host,
        ];
        candidates
            .iter()
            .flatten()
            .find(|path| path.is_file())
            .cloned()
            .ok_or_else(|| EngineError::Missing(missing_message(&candidates)))
    }

    /// Run a one-shot command and return its result message.
    ///
    /// # Errors
    ///
    /// Returns an [`EngineError`] when the engine is missing, fails or does not answer.
    pub async fn run(&self, args: &[String]) -> Result<EngineMessage, EngineError> {
        let mut running = self.spawn(args)?;
        let answer = match running.next().await {
            Ok(Some(EngineMessage::Failure {
                error_kind,
                message,
            })) => Err(EngineError::Failed {
                kind: error_kind,
                message,
            }),
            Ok(Some(message)) => Ok(message),
            Ok(None) => Err(EngineError::NoAnswer(format!(" to `{}`", args.join(" ")))),
            Err(error) => Err(error),
        };
        running.finish().await;
        answer
    }

    /// Spawn `args` with piped stdin and stdout.
    ///
    /// # Errors
    ///
    /// Returns an [`EngineError`] when the engine is missing or cannot start.
    pub fn spawn(&self, args: &[String]) -> Result<RunningEngine, EngineError> {
        let executable = self.locate()?;
        debug!(engine = %executable.display(), ?args, "starting the .NET profiling engine");
        let mut child = Command::new(&executable)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| EngineError::Spawn(error.to_string()))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().map(BufReader::new);
        Ok(RunningEngine {
            child,
            stdin,
            stdout,
        })
    }
}

/// A spawned engine command.
#[derive(Debug)]
pub struct RunningEngine {
    /// Owns the engine process so dropping this handle also terminates it.
    child: Child,
    /// Remains available until a CPU session transfers it to its stop signal.
    stdin: Option<ChildStdin>,
    /// Buffers the NDJSON response stream until the engine closes it.
    stdout: Option<BufReader<ChildStdout>>,
}

impl RunningEngine {
    /// The next message, or `None` when the engine closed stdout.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Io`] when a line cannot be read or decoded.
    pub async fn next(&mut self) -> Result<Option<EngineMessage>, EngineError> {
        let Some(stdout) = self.stdout.as_mut() else {
            return Ok(None);
        };
        let mut line = String::new();
        loop {
            line.clear();
            let read = stdout
                .read_line(&mut line)
                .await
                .map_err(|error| EngineError::Io(error.to_string()))?;
            if read == 0 {
                return Ok(None);
            }
            if line.trim().is_empty() {
                continue;
            }
            return serde_json::from_str(&line)
                .map(Some)
                .map_err(|error| EngineError::Io(error.to_string()));
        }
    }

    /// Hand over the engine's stdin: the CPU command ends its recording on a
    /// line there, so whoever decides when to stop owns it while this side
    /// keeps reading what the engine streams.
    pub fn stop_signal(&mut self) -> StopSignal {
        StopSignal {
            stdin: self.stdin.take(),
        }
    }

    /// Close stdin and wait for the engine to exit.
    pub async fn finish(mut self) {
        drop(self.stdin.take());
        if let Err(error) = self.child.wait().await {
            warn!(%error, "waiting for the .NET profiling engine failed");
        }
    }
}

/// The CPU command's stop line, sent at most once.
#[derive(Debug)]
pub struct StopSignal {
    /// Provides exclusive command input while a CPU recording is active.
    stdin: Option<ChildStdin>,
}

impl StopSignal {
    /// Close the current CPU segment without ending the recording.
    ///
    /// # Errors
    /// Returns an I/O error if the engine has stopped accepting commands.
    pub async fn snapshot(&mut self) -> Result<(), EngineError> {
        self.write(b"snapshot\n").await
    }

    /// Tell the engine to end its recording; dropping the signal closes stdin.
    pub async fn send(mut self) {
        if let Err(error) = self.write(b"stop\n").await {
            warn!(%error, "cannot signal the .NET profiling engine");
        }
    }

    /// Write and flush one complete command so the engine acts on it immediately.
    async fn write(&mut self, command: &[u8]) -> Result<(), EngineError> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(EngineError::Io("engine command input is closed".to_owned()));
        };
        stdin
            .write_all(command)
            .await
            .map_err(|error| EngineError::Io(error.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|error| EngineError::Io(error.to_string()))
    }
}

/// Add the executable suffix required by the host operating system.
fn executable_name() -> String {
    if cfg!(windows) {
        format!("{ENGINE_STEM}.exe")
    } else {
        ENGINE_STEM.to_owned()
    }
}

/// Include every attempted location so a missing engine is actionable.
fn missing_message(candidates: &[Option<PathBuf>]) -> String {
    let looked = candidates
        .iter()
        .flatten()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{ENGINE_STEM} was not found (looked at: {looked}). Set {ENGINE_ENV} or place the engine beside the language server."
    )
}

/// Whether `path` names the engine executable.
#[must_use]
pub fn is_engine_path(path: &Path) -> bool {
    path.file_stem().is_some_and(|stem| stem == ENGINE_STEM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configured_engine_wins_when_it_exists() {
        let dir = tempfile::tempdir().ok();
        assert!(dir.is_some(), "tempdir");
        let Some(dir) = dir else {
            return;
        };
        let path = dir.path().join("engine");
        let _ = std::fs::write(&path, b"");
        assert_eq!(Engine::new(Some(path.clone())).locate().ok(), Some(path));
    }

    #[test]
    fn a_missing_engine_names_every_place_it_looked() {
        let error = Engine::new(Some(PathBuf::from("/definitely/not/here")))
            .locate()
            .err();
        let message = error.map(|e| e.to_string()).unwrap_or_default();
        assert!(message.contains("/definitely/not/here"), "{message}");
        assert!(message.contains(ENGINE_ENV), "{message}");
    }

    #[test]
    fn engine_paths_are_recognised_by_stem() {
        assert!(is_engine_path(Path::new("/x/lsp-profiler-dotnet-engine")));
        assert!(is_engine_path(Path::new("lsp-profiler-dotnet-engine.exe")));
        assert!(!is_engine_path(Path::new("/x/dotnet")));
    }
}
