//! Real CLI depth, native events and natural process exit. [LSP-PULL-WATCH]
use serde_json::Value;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[path = "../../../test-support/native_events.rs"]
mod native_events;
use native_events::NativeEvents;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn rendered_paths(batch: &Value) -> Vec<(&str, PathBuf)> {
    assert_eq!(batch["kind"], "batch");
    let events = batch["events"].as_array().expect("native events");
    assert!(
        !events.is_empty(),
        "a successful batch contains actual OS events"
    );
    events.iter().flat_map(event_paths).collect()
}

fn event_paths(event: &Value) -> impl Iterator<Item = (&str, PathBuf)> {
    let kind = event["kind"].as_str().expect("native event kind");
    event["paths"]
        .as_array()
        .expect("native event paths")
        .iter()
        .map(move |path| (kind, PathBuf::from(path.as_str().expect("native path"))))
}

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

fn assert_scope(paths: &[(&str, PathBuf)], root: &Path, child: &Path, recursive: bool) {
    assert!(paths.iter().all(|(_, path)| path.starts_with(root)));
    if !recursive {
        assert!(
            paths.iter().all(|(_, path)| path != child),
            "descendant files stay unwatched"
        );
        assert!(paths.iter().all(|(_, path)| path.parent() == Some(root)));
    }
}

fn change_file(path: &Path, removed: bool) -> std::io::Result<()> {
    if removed {
        std::fs::remove_file(path)
    } else {
        std::fs::write(path, b"{\"value\":1}")
    }
}

async fn exercise_depth(recursive: bool) -> TestResult {
    let (dir, child) = fixture()?;
    let root = dir.path();
    let mut watcher = if recursive {
        NativeEvents::watcher(root, 2).await
    } else {
        NativeEvents::with_depth(root, 2, false).await
    };
    for removed in [false, true] {
        change_file(&child, removed)?;
        if !recursive {
            change_file(&root.join("config.json"), removed)?;
        }
        assert_batch(&watcher.next().await, root, &child, recursive, removed);
    }
    watcher.finish().await;
    Ok(())
}

fn fixture() -> Result<(TempDir, PathBuf), std::io::Error> {
    let dir = TempDir::new()?;
    let nested = dir.path().join("child");
    std::fs::create_dir(&nested)?;
    let child = nested.join("sample.json");
    Ok((dir, child))
}

#[tokio::test]
async fn shallow_watcher_observes_direct_lifecycle_and_excludes_descendants() -> TestResult {
    exercise_depth(false).await
}

#[tokio::test]
async fn recursive_watcher_preserves_descendant_lifecycle() -> TestResult {
    exercise_depth(true).await
}
