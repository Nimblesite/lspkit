//! Real FIFO source reads on Unix. [LSP-PULL-CONCURRENCY]

use super::RetryWriter;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub struct NativeSource(PathBuf);

pub struct HeldRead {
    writer: File,
    source: PathBuf,
}

impl NativeSource {
    pub fn new(source: &Path) -> Self {
        let made = std::process::Command::new("mkfifo")
            .arg(source)
            .output()
            .expect("create native source FIFO");
        assert!(made.status.success(), "native source FIFO was created");
        Self(source.to_owned())
    }

    pub fn writer(&mut self) -> HeldRead {
        HeldRead {
            writer: open_writer(&self.0).expect("real analyzer opened its native source reader"),
            source: self.0.clone(),
        }
    }
}

impl HeldRead {
    pub fn release(self, retry: bool) -> Option<RetryWriter> {
        supply_original(self.writer).expect("supply actual old parser input");
        retry.then(|| -> RetryWriter {
            Box::new(move || supply_original(open_writer(&self.source)?))
        })
    }
}

fn open_writer(source: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).open(source)
}

fn supply_original(mut writer: File) -> io::Result<()> {
    writer.write_all(b"{\"unfinished\":")
}
