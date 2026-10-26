//! Actual source I/O is the concurrency barrier. [LSP-PULL-CONCURRENCY]

use super::{bulk_workspace, initialize_at, uri_for, ReferenceClient};
use std::io;
use std::thread::JoinHandle;
use tempfile::TempDir;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(unix)]
use unix as native;
#[cfg(windows)]
use windows as native;

pub use native::HeldRead;
pub type RetryWriter = Box<dyn FnOnce() -> io::Result<()> + Send>;
pub type RunningRetryWriter = JoinHandle<io::Result<()>>;

/// Every native read contract starts one real protocol session against its owner.
pub async fn source_session(count: usize) -> (HeldSource, ReferenceClient) {
    let fixture = HeldSource::new(count);
    let mut client = ReferenceClient::spawn();
    initialize_at(&mut client, fixture.workspace.path()).await;
    (fixture, client)
}

/// The real analyzer cannot finish this source read until its owner releases it.
pub struct HeldSource {
    pub workspace: TempDir,
    pub source_uri: String,
    pub good_uri: String,
    native: native::NativeSource,
}

impl HeldSource {
    /// Reuse the real workspace builder; establish the barrier before watching.
    pub fn new(count: usize) -> Self {
        let workspace = bulk_workspace(count);
        let source = workspace.path().join("waiting.json");
        Self {
            native: native::NativeSource::new(&source),
            source_uri: uri_for(&source),
            good_uri: uri_for(workspace.path().join("scan-0000.json")),
            workspace,
        }
    }

    /// Returns only after the actual backend has attempted its native source read.
    pub fn writer(&mut self) -> HeldRead {
        self.native.writer()
    }
}

/// Preserve the old broken source while releasing the native read barrier.
pub fn release_original(writer: HeldRead) {
    assert!(writer.release(false).is_none());
}

/// The streamed retry finishing proves the second native source read completed.
pub fn join_retry(writer: Option<RunningRetryWriter>) {
    if let Some(writer) = writer {
        writer
            .join()
            .expect("native retry writer exits")
            .expect("supply real retry input");
    }
}

/// Start only after a real retry partial proves the original native reader closed.
pub fn start_retry(writer: Option<RetryWriter>) -> Option<RunningRetryWriter> {
    writer.map(|supply| {
        std::thread::Builder::new()
            .spawn(supply)
            .expect("start real retry FIFO writer after original reader closure")
    })
}
