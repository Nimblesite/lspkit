//! Overflow controls are linked only into the native loss suite. [LSP-PULL-WATCH]
use serde_json::Value;
use tokio::io::AsyncReadExt;

use super::native_events::{reference_program, NativeEvents};

#[path = "stopped_process.rs"]
mod stopped_process;

impl NativeEvents {
    /// Pause only this ready native watcher, with automatic continuation on failure.
    pub async fn suspend(&self) -> stopped_process::Stopped {
        let pid = self.child.id().expect("owned native reference is alive");
        stopped_process::suspend(pid, reference_program::READ_GUARD).await
    }

    /// Parse the actual structured diagnostic stream after natural process exit.
    pub async fn diagnostics(&mut self) -> Vec<Value> {
        let mut output = String::new();
        self.child
            .stderr
            .take()
            .expect("native diagnostic pipe")
            .read_to_string(&mut output)
            .await
            .expect("read native diagnostics");
        output
            .lines()
            .map(|line| serde_json::from_str(line).expect("structured diagnostic"))
            .collect()
    }
}
