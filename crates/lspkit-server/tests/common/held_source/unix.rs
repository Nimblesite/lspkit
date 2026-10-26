//! Real FIFO source reads on Unix. [LSP-PULL-CONCURRENCY]

use super::{RetryWriter, StepResult};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The FIFO source whose open write end blocks the real analyzer.
pub struct NativeSource(PathBuf);

/// The held write end of the FIFO, controlled by the test.
pub struct HeldRead {
    /// The open FIFO writer end.
    writer: File,
    /// The FIFO path a retry writer must reopen.
    source: PathBuf,
}

impl NativeSource {
    /// Create the real native source as a FIFO the analyzer will block on.
    pub fn new(source: &Path) -> StepResult<Self> {
        let made = std::process::Command::new("mkfifo").arg(source).output()?;
        if !made.status.success() {
            return Err(format!("native source FIFO was created: {}", made.status).into());
        }
        Ok(Self(source.to_owned()))
    }

    /// Take the write end; this is the barrier the test controls.
    pub fn writer(&mut self) -> StepResult<HeldRead> {
        Ok(HeldRead {
            writer: open_writer(&self.0)?,
            source: self.0.clone(),
        })
    }
}

impl HeldRead {
    /// Supply the old broken input, optionally arming a retry writer.
    pub fn release(self, retry: bool) -> StepResult<Option<RetryWriter>> {
        supply_original(self.writer)?;
        Ok(retry.then(|| -> RetryWriter {
            Box::new(move || supply_original(open_writer(&self.source)?))
        }))
    }
}

/// Open the FIFO write end; blocks until the reader arrives.
fn open_writer(source: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).open(source)
}

/// Write the actual broken JSON the old parser must consume.
fn supply_original(mut writer: File) -> io::Result<()> {
    writer.write_all(b"{\"unfinished\":")
}
