//! An NTFS oplock holds the actual Rust file open. [LSP-PULL-CONCURRENCY]

use super::RetryWriter;
use std::io::BufReader;
use std::path::Path;
use std::process::{Child, ChildStdout};

#[path = "../../../../../test-support/windows-oplock/owner.rs"]
mod owner;

pub struct NativeSource(Option<HeldRead>);

pub struct HeldRead {
    child: Child,
    output: BufReader<ChildStdout>,
}

impl NativeSource {
    pub fn new(source: &Path) -> Self {
        std::fs::write(source, b"{\"unfinished\":").expect("write real held source");
        let mut child = spawn_owner(source);
        let output = BufReader::new(child.stdout.take().expect("oplock stdout"));
        let mut held = HeldRead { child, output };
        held.state("ready");
        Self(Some(held))
    }

    pub fn writer(&mut self) -> HeldRead {
        let mut held = self.0.take().expect("one held native reader");
        held.state("blocked");
        held
    }
}

/// Run the owned native fixture invisibly, with no persistent policy changes.
fn spawn_owner(source: &Path) -> Child {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-support/windows-oplock/hold.ps1");
    owner::spawn(&script, source)
}

impl HeldRead {
    fn state(&mut self, expected: &str) {
        owner::state(&mut self.output, expected).expect("expected native oplock state");
    }

    pub fn release(mut self, _retry: bool) -> Option<RetryWriter> {
        drop(self.child.stdin.take());
        self.state("released");
        assert!(self.child.wait().expect("oplock owner exits").success());
        None
    }
}
