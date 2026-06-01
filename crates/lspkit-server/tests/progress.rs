//! End-to-end tests for [`lspkit_server::progress`].

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use lspkit_server::{NoopReporter, ProgressReporter, ProgressSink, ProgressUpdate};

struct Recorder {
    updates: Mutex<Vec<ProgressUpdate>>,
}

#[async_trait]
impl ProgressReporter for Recorder {
    async fn report(&self, update: ProgressUpdate) {
        if let Ok(mut guard) = self.updates.lock() {
            guard.push(update);
        }
    }
}

#[tokio::test]
async fn noop_sink_drops_updates() {
    let sink = ProgressSink::noop();
    sink.report("hello", Some(50)).await;
    sink.finish("done").await;
}

#[tokio::test]
async fn report_forwards_with_token_and_done_false() {
    let recorder = Arc::new(Recorder {
        updates: Mutex::new(Vec::new()),
    });
    let sink = ProgressSink::new(Arc::clone(&recorder) as Arc<dyn ProgressReporter>, "tok-1");
    sink.report("step 1", Some(25)).await;
    sink.report("step 2", None).await;

    let updates = recorder.updates.lock().expect("lock");
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].token, "tok-1");
    assert_eq!(updates[0].message, "step 1");
    assert_eq!(updates[0].percentage, Some(25));
    assert!(!updates[0].done);
    assert_eq!(updates[1].percentage, None);
    assert!(!updates[1].done);
}

#[tokio::test]
async fn finish_sets_percentage_100_and_done_true() {
    let recorder = Arc::new(Recorder {
        updates: Mutex::new(Vec::new()),
    });
    let sink = ProgressSink::new(Arc::clone(&recorder) as Arc<dyn ProgressReporter>, "tok-2");
    sink.finish("complete").await;

    let updates = recorder.updates.lock().expect("lock");
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].token, "tok-2");
    assert_eq!(updates[0].message, "complete");
    assert_eq!(updates[0].percentage, Some(100));
    assert!(updates[0].done);
}

#[tokio::test]
async fn noop_reporter_can_be_used_directly() {
    let reporter = NoopReporter;
    reporter.report(ProgressUpdate::new("x", "y", None)).await;
}

#[tokio::test]
async fn sink_clone_shares_reporter() {
    let recorder = Arc::new(Recorder {
        updates: Mutex::new(Vec::new()),
    });
    let sink = ProgressSink::new(Arc::clone(&recorder) as Arc<dyn ProgressReporter>, "tok");
    let cloned = sink.clone();
    sink.report("a", None).await;
    cloned.report("b", None).await;
    assert_eq!(recorder.updates.lock().expect("lock").len(), 2);
}

#[tokio::test]
async fn sink_debug_shows_token() {
    let sink = ProgressSink::noop();
    assert!(format!("{sink:?}").contains("ProgressSink"));
}

#[test]
fn noop_reporter_default_and_copy() {
    let lhs = NoopReporter;
    let rhs = NoopReporter;
    assert_eq!(format!("{lhs:?}"), format!("{rhs:?}"));
}

#[test]
fn progress_update_new_sets_done_false() {
    let upd = ProgressUpdate::new("t", "m", Some(10));
    assert_eq!(upd.token, "t");
    assert_eq!(upd.message, "m");
    assert_eq!(upd.percentage, Some(10));
    assert!(!upd.done);
}

#[test]
fn progress_update_final_sets_done_true_and_full_percentage() {
    let upd = ProgressUpdate::final_update("tok", "fin");
    assert_eq!(upd.percentage, Some(100));
    assert!(upd.done);
}

#[test]
fn progress_update_clone_works() {
    let upd = ProgressUpdate::new("t", "m", None);
    let copy = upd.clone();
    assert_eq!(copy.token, "t");
}
