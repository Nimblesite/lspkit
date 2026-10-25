//! Interactive native watcher journal from the actual compiled reference CLI.

use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout};
use tokio::time::timeout;

#[path = "reference_program.rs"]
pub(super) mod reference_program;

/// Preserve the original native watcher responsiveness guard.
const GUARD: Duration = Duration::from_secs(10);

/// Only the OS child and its rendered output are visible to these assertions.
pub struct NativeEvents {
    pub(super) child: Child,
    lines: Lines<BufReader<ChildStdout>>,
}

impl NativeEvents {
    /// Wait for actual native watcher readiness before touching fixture files.
    pub async fn watcher(root: &Path, batches: usize) -> Self {
        Self::with_depth(root, batches, true).await
    }

    /// Exercise the real CLI's depth choice with the same readiness and exit guards.
    pub async fn with_depth(root: &Path, batches: usize, recursive: bool) -> Self {
        let root = root.to_str().expect("native fixture argument");
        let batches = batches.to_string();
        let mode = if recursive {
            "--watch"
        } else {
            "--watch-shallow"
        };
        let mut child = reference_program::spawn_reference(&[mode, root, &batches])
            .expect("spawn real native watcher CLI");
        let lines = BufReader::new(child.stdout.take().expect("watcher stdout")).lines();
        let mut session = Self { child, lines };
        let ready = session.next().await;
        assert_eq!(ready["kind"], "watching");
        assert_eq!(ready["recursive"], recursive);
        session
    }

    /// Read an actual rendered event within the unchanged liveness guard.
    pub async fn next(&mut self) -> Value {
        let line = timeout(GUARD, self.lines.next_line())
            .await
            .expect("coalesced trigger")
            .expect("read watcher journal")
            .expect("trigger before the channel closes");
        serde_json::from_str(&line).expect("structured native event")
    }

    /// Explicit release, natural child termination and EOF are all observable.
    pub async fn finish(&mut self) {
        assert_eq!(self.next().await["kind"], "released");
        let status = timeout(reference_program::READ_GUARD, self.child.wait())
            .await
            .expect("native watcher exits")
            .expect("native process status");
        assert!(status.success(), "native watcher failed: {status:?}");
        assert!(timeout(GUARD, self.lines.next_line())
            .await
            .expect("watcher EOF")
            .expect("read watcher EOF")
            .is_none());
    }
}
