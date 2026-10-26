//! An NTFS oplock holds the actual Rust file open. [LSP-PULL-CONCURRENCY]

use super::{RetryWriter, StepResult};
use std::io::BufReader;
use std::path::Path;
use std::process::{Child, ChildStdout};

#[path = "../../../../../test-support/windows-oplock/owner.rs"]
mod owner;

/// The oplock-owning child acting as the held native source.
pub struct NativeSource(Option<HeldRead>);

/// The owner child and its state-line reader.
pub struct HeldRead {
    /// The PowerShell owner holding the oplock.
    child: Child,
    /// Buffered owner stdout carrying JSON state lines.
    output: BufReader<ChildStdout>,
}

impl NativeSource {
    /// Write the real held source and start the owner holding it.
    pub fn new(source: &Path) -> StepResult<Self> {
        std::fs::write(source, b"{\"unfinished\":")?;
        let mut child = spawn_owner(source);
        let stdout = child.stdout.take().ok_or("oplock stdout")?;
        let mut held = HeldRead {
            child,
            output: BufReader::new(stdout),
        };
        held.state("ready")?;
        Ok(Self(Some(held)))
    }

    /// Take the single held reader; the barrier is now the test's.
    pub fn writer(&mut self) -> StepResult<HeldRead> {
        let mut held = self.0.take().ok_or("one held native reader")?;
        held.state("blocked")?;
        Ok(held)
    }
}

/// Run the owned native fixture invisibly, with no persistent policy changes.
fn spawn_owner(source: &Path) -> Child {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-support/windows-oplock/hold.ps1");
    owner::spawn(&script, source)
}

impl HeldRead {
    /// The owner reports the named state before the step completes.
    fn state(&mut self, expected: &str) -> StepResult<()> {
        owner::state(&mut self.output, expected)?;
        Ok(())
    }

    /// Release the oplock; this platform has no retry writer.
    pub fn release(mut self, _retry: bool) -> StepResult<Option<RetryWriter>> {
        drop(self.child.stdin.take());
        self.state("released")?;
        if !self.child.wait()?.success() {
            return Err("oplock owner exits".into());
        }
        Ok(None)
    }
}
