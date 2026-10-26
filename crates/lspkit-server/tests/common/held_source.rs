//! Actual source I/O is the concurrency barrier. [LSP-PULL-CONCURRENCY]

use std::io;
use std::thread::JoinHandle;
use tempfile::TempDir;

use super::StepResult;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(unix)]
use unix as native;
#[cfg(windows)]
use windows as native;

pub use native::HeldRead;

/// Deferred input supply for the analyzer's second native read attempt.
pub type RetryWriter = Box<dyn FnOnce() -> io::Result<()> + Send>;
/// A retry writer running on its own OS thread.
pub type RunningRetryWriter = JoinHandle<io::Result<()>>;

/// The real analyzer cannot finish this source read until its owner releases it.
pub struct HeldSource {
    /// The watched workspace fixture the held source lives in.
    pub workspace: TempDir,
    /// The URI of the source whose native read is held open.
    pub source_uri: String,
    /// A freely readable sibling source in the same workspace.
    pub good_uri: String,
    native: native::NativeSource,
}

impl HeldSource {
    /// Reuse the real workspace builder; establish the barrier before watching.
    pub fn new(count: usize) -> StepResult<Self> {
        let workspace = super::bulk_workspace(count)?;
        let source = workspace.path().join("waiting.json");
        Ok(Self {
            native: native::NativeSource::new(&source)?,
            source_uri: super::uri_for(&source)?,
            good_uri: super::uri_for(workspace.path().join("scan-0000.json"))?,
            workspace,
        })
    }

    /// Returns only after the actual backend has attempted its native source read.
    pub fn writer(&mut self) -> StepResult<HeldRead> {
        self.native.writer()
    }
}

/// Preserve the old broken source while releasing the native read barrier.
pub fn release_original(writer: HeldRead) -> StepResult<()> {
    match writer.release(false)? {
        None => Ok(()),
        Some(_) => Err("release keeps no retry writer".into()),
    }
}

/// The streamed retry finishing proves the second native source read completed.
pub fn join_retry(writer: Option<RunningRetryWriter>) -> StepResult<()> {
    if let Some(writer) = writer {
        writer
            .join()
            .map_err(|joined| format!("native retry writer exits: {joined:?}"))??;
    }
    Ok(())
}

/// Start only after a real retry partial proves the original native reader closed.
pub fn start_retry(writer: Option<RetryWriter>) -> StepResult<Option<RunningRetryWriter>> {
    Ok(writer
        .map(|supply| std::thread::Builder::new().spawn(supply))
        .transpose()?)
}

/// A held-source fixture plus one initialized real session against its owner.
/// Expands inside the calling `#[test]`.
macro_rules! source_session {
    ($count:expr) => {{
        let fixture =
            crate::common::held_source::HeldSource::new($count).expect("held-source fixture");
        let mut client = crate::common::ReferenceClient::spawn().expect("spawn reference server");
        crate::common::initialize_session!(client, fixture.workspace.path());
        (fixture, client)
    }};
}
pub(crate) use source_session;
