//! Actual CLI watcher batches and natural ownership release. [LSP-PULL-WATCH]

use serde_json::Value;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use common::at;
use native_events::NativeEvents;

mod common;
#[path = "../../../test-support/native_events.rs"]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "shared test-only module is outside Clippy's test-function detection"
)]
mod native_events;

#[cfg(target_os = "linux")]
#[path = "../../../test-support/native_events_overflow.rs"]
#[expect(
    clippy::expect_used,
    reason = "shared test-only module is outside Clippy's test-function detection"
)]
mod native_events_overflow;

#[cfg(target_os = "linux")]
#[path = "../../../test-support/native_overflow.rs"]
#[expect(
    clippy::expect_used,
    reason = "shared test-only module is outside Clippy's test-function detection"
)]
mod native_overflow;

/// Write one real JSON file into the watched root.
fn write_sample(dir: &Path, name: &str) {
    let result = std::fs::write(dir.join(format!("{name}.json")), b"{\"watched\": true}");
    assert!(result.is_ok(), "write sample failed: {:?}", result.err());
}

/// Preserve nonempty batches and every original native path ownership assertion.
fn assert_batch(batch: &Value, root: &Path) {
    assert_eq!(at(batch, "/kind"), "batch");
    let events = at(batch, "/events").as_array();
    assert!(events.is_some(), "native events must be an array");
    assert_ne!(events.map_or(0, Vec::len), 0);
    let paths_are_arrays = events
        .into_iter()
        .flatten()
        .all(|event| at(event, "/paths").is_array());
    assert!(paths_are_arrays, "native paths must be arrays");
    let owned_paths = events.into_iter().flatten().all(|event| {
        at(event, "/paths")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|path| PathBuf::from(path.as_str().unwrap_or_default()))
            .all(|path| path.starts_with(root))
    });
    assert!(owned_paths, "batches carry real watched paths");
}

/// Both lifetimes finish with the same actual write, batch assertions and release.
async fn final_batch(watcher: &mut NativeEvents, root: &Path, name: &str) {
    write_sample(root, name);
    assert_batch(&watcher.next().await, root);
    watcher.finish().await;
}

#[tokio::test]
async fn coalesced_watcher_batches_real_disk_events() {
    let dir = TempDir::new().expect("tempdir");
    let mut watcher = NativeEvents::watcher(dir.path(), 2).await;

    for name in ["a", "b", "c", "d", "e", "f"] {
        write_sample(dir.path(), name);
    }
    assert_batch(&watcher.next().await, dir.path());
    final_batch(&mut watcher, dir.path(), "second-batch").await;
}

#[tokio::test]
async fn dropping_the_coalesced_watcher_returns_ownership() {
    let dir = TempDir::new().expect("tempdir");
    let mut original = NativeEvents::watcher(dir.path(), 0).await;
    original.finish().await;
    let mut replacement = NativeEvents::watcher(dir.path(), 1).await;
    final_batch(&mut replacement, dir.path(), "after-drop").await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn native_queue_overflow_is_reported_and_fresh_events_still_arrive() {
    let dir = TempDir::new().expect("native overflow root");
    let mut watcher = NativeEvents::watcher(dir.path(), 2).await;
    let stopped = watcher.suspend().await;
    native_overflow::saturate(dir.path(), "queued");
    stopped.resume();
    assert_batch(&watcher.next().await, dir.path());
    final_batch(&mut watcher, dir.path(), "after-native-overflow").await;
    let diagnostics = watcher.diagnostics().await;
    assert!(
        diagnostics.iter().any(|event| {
            at(event, "/level") == "WARN"
                && at(event, "/fields/message") == "Filesystem observation failed"
                && at(event, "/fields/affected_paths") == 0
        }),
        "real observation loss cannot be silent"
    );
}
