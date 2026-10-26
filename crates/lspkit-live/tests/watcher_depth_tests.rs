//! Real CLI depth, native events and natural process exit. [LSP-PULL-WATCH]
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

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
type DepthRun = (TempDir, PathBuf, Vec<Value>);

/// Flatten actual native event paths while checking the batch envelope.
fn rendered_paths(batch: &Value) -> Vec<(&str, PathBuf)> {
    assert_eq!(at(batch, "/kind"), "batch");
    let events = at(batch, "/events").as_array();
    assert!(events.is_some(), "native events must be an array");
    assert!(
        events.is_some_and(|items| !items.is_empty()),
        "a successful batch contains actual OS events"
    );
    events.into_iter().flatten().flat_map(event_paths).collect()
}

/// Associate every native path with its coarse event kind.
fn event_paths(event: &Value) -> impl Iterator<Item = (&str, PathBuf)> {
    let kind = at(event, "/kind").as_str().unwrap_or_default();
    let paths = at(event, "/paths").as_array();
    assert!(paths.is_some(), "native event paths must be an array");
    paths
        .into_iter()
        .flatten()
        .map(move |path| (kind, PathBuf::from(path.as_str().unwrap_or_default())))
}

/// The expected lifecycle kind and target appear without escaping the watched scope.
fn assert_batch(batch: &Value, root: &Path, child: &Path, recursive: bool, removed: bool) {
    let paths = rendered_paths(batch);
    let target = if recursive {
        child.to_path_buf()
    } else {
        root.join("config.json")
    };
    let kind = if removed { "removed" } else { "touched" };
    assert!(paths
        .iter()
        .any(|(event, path)| *event == kind && *path == target));
    assert_scope(&paths, root, child, recursive);
}

/// Non-recursive watching excludes descendants; both modes retain root ownership.
fn assert_scope(paths: &[(&str, PathBuf)], root: &Path, child: &Path, recursive: bool) {
    assert!(paths.iter().all(|(_, path)| path.starts_with(root)));
    if !recursive {
        assert!(
            paths.iter().all(|(_, path)| path != child),
            "descendant files stay unwatched"
        );
        assert!(
            paths.iter().all(|(_, path)| path.parent() == Some(root)),
            "every shallow event names a direct child of {}: {paths:?}",
            root.display()
        );
    }
}

/// Apply either removal or modification to a real watched file.
fn change_file(path: &Path, removed: bool) -> std::io::Result<()> {
    if removed {
        std::fs::remove_file(path)
    } else {
        std::fs::write(path, b"{\"value\":1}")
    }
}

/// Drive both lifecycle changes and collect their real batches for assertions.
async fn exercise_depth(recursive: bool) -> TestResult<DepthRun> {
    let (dir, child) = fixture()?;
    let root = dir.path();
    let mut watcher = if recursive {
        NativeEvents::watcher(root, 2).await
    } else {
        NativeEvents::with_depth(root, 2, false).await
    };
    let mut batches = Vec::new();
    for removed in [false, true] {
        change_file(&child, removed)?;
        if !recursive {
            change_file(&root.join("config.json"), removed)?;
        }
        batches.push(watcher.next().await);
    }
    watcher.finish().await;
    Ok((dir, child, batches))
}

/// Construct a real nested source path beneath a temporary watched root.
fn fixture() -> Result<(TempDir, PathBuf), std::io::Error> {
    let dir = TempDir::new()?;
    let nested = dir.path().join("child");
    std::fs::create_dir(&nested)?;
    let child = nested.join("sample.json");
    Ok((dir, child))
}

#[tokio::test]
async fn shallow_watcher_observes_direct_lifecycle_and_excludes_descendants() {
    let (dir, child, batches) = exercise_depth(false).await.expect("shallow watcher run");
    for (removed, batch) in [false, true].into_iter().zip(&batches) {
        assert_batch(batch, dir.path(), &child, false, removed);
    }
}

#[tokio::test]
async fn recursive_watcher_preserves_descendant_lifecycle() {
    let (dir, child, batches) = exercise_depth(true).await.expect("recursive watcher run");
    for (removed, batch) in [false, true].into_iter().zip(&batches) {
        assert_batch(batch, dir.path(), &child, true, removed);
    }
}
