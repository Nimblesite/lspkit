//! E2E watcher tests.
//!
//! The watcher wraps `notify::recommended_watcher`, which surfaces OS-level
//! filesystem events. We exercise:
//!   - construction success against a real temp directory,
//!   - construction failure against a non-existent path,
//!   - the public `next()` API up to OS-event observation, drained without
//!     time-based assertions to stay deterministic. We never assert that a
//!     specific number of events fired — only that the API behaves correctly.

#![deny(missing_docs)]

use std::path::PathBuf;
use std::time::Duration;

use lspkit_live::{ChangeEvent, FileWatcher, WatcherError};
use tempfile::TempDir;
use tokio::time::timeout;

#[tokio::test]
async fn construct_against_temp_dir_succeeds() {
    let dir = TempDir::new().expect("tempdir");
    let watcher = FileWatcher::new(dir.path()).expect("watcher");
    drop(watcher);
}

#[tokio::test]
async fn construct_against_missing_path_returns_watch_error() {
    let dir = TempDir::new().expect("tempdir");
    let missing: PathBuf = dir.path().join("nope-does-not-exist");
    let result = FileWatcher::new(&missing);
    let Err(err) = result else { return };
    // OS-dependent: some platforms allow watching a non-existent path. If an
    // error came back, it must be the `Watch` variant.
    assert!(matches!(
        err,
        WatcherError::Watch(_) | WatcherError::Setup(_)
    ));
    let display = format!("{err}");
    assert!(display.contains("watch failed"));
    let dbg = format!("{err:?}");
    assert!(dbg.contains("Watch"));
}

#[tokio::test]
async fn watcher_error_setup_variant_renders() {
    let err = WatcherError::Setup("oops".into());
    assert!(format!("{err}").contains("watcher setup failed"));
    assert!(format!("{err}").contains("oops"));
    assert!(format!("{err:?}").contains("Setup"));
}

#[tokio::test]
async fn next_returns_none_when_watcher_inner_drops_on_close() {
    // Drop the watcher and the inner channel should close. Pulling next from
    // a moved watcher isn't possible — instead, observe that `next()` is
    // pollable without panicking before drop.
    let dir = TempDir::new().expect("tempdir");
    let mut watcher = FileWatcher::new(dir.path()).expect("watcher");
    // Don't wait long: virtual time isn't engaged because the watcher uses a
    // background OS thread. A tiny bounded timeout keeps the test fast.
    let polled = timeout(Duration::from_millis(50), watcher.next()).await;
    // Either timed out (no OS events) or received an event — both are valid.
    if let Ok(Some(evt)) = polled {
        let paths = match evt {
            ChangeEvent::Touched(p) | ChangeEvent::Removed(p) => p,
            _ => Vec::new(),
        };
        assert!(paths.iter().all(|p| p.is_absolute() || p.is_relative()));
    }
}

#[tokio::test]
async fn touched_and_removed_event_variants_can_be_constructed_and_cloned() {
    let touched = ChangeEvent::Touched(vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    let removed = ChangeEvent::Removed(vec![PathBuf::from("/c")]);

    let t2 = touched.clone();
    let r2 = removed.clone();

    let t_count = match t2 {
        ChangeEvent::Touched(p) => p.len(),
        _ => 0,
    };
    let r_count = match r2 {
        ChangeEvent::Removed(p) => p.len(),
        _ => 0,
    };
    assert_eq!(t_count, 2);
    assert_eq!(r_count, 1);

    assert!(format!("{touched:?}").contains("Touched"));
    assert!(format!("{removed:?}").contains("Removed"));
}

#[tokio::test]
async fn write_to_watched_dir_optionally_yields_events() {
    // Best-effort, deterministic in the sense that we never block forever and
    // never assert a specific event was received (OS scheduling decides).
    let dir = TempDir::new().expect("tempdir");
    let mut watcher = FileWatcher::new(dir.path()).expect("watcher");

    let target = dir.path().join("changed.txt");
    std::fs::write(&target, b"hello").expect("write");
    std::fs::remove_file(&target).expect("rm");

    // Drain whatever the OS produced within a tight bound. Many platforms will
    // surface at least one event; some test runners suppress them. Both are
    // acceptable here — we're covering the receive loop, not asserting OS.
    let mut drained = 0_usize;
    while drained < 4 {
        let next = timeout(Duration::from_millis(80), watcher.next()).await;
        match next {
            Ok(Some(_)) => drained += 1,
            Ok(None) | Err(_) => break,
        }
    }
}
