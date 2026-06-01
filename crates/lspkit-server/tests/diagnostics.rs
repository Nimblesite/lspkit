//! End-to-end tests for [`lspkit_server::diagnostics`].

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use lspkit::Generation;
use lspkit_server::{
    diagnostics::document_uri_from_path, Diagnostic, DiagnosticsBatch, DiagnosticsBus,
    DiagnosticsSink, Severity,
};
use lspkit_vfs::DocumentUri;

/// Test sink that records every batch it receives.
struct Recorder {
    batches: Mutex<Vec<DiagnosticsBatch>>,
}

#[async_trait]
impl DiagnosticsSink for Recorder {
    async fn publish(&self, batch: DiagnosticsBatch) {
        if let Ok(mut guard) = self.batches.lock() {
            guard.push(batch);
        }
    }
}

fn make_batch(uri: &str, generation: u64, count: usize) -> DiagnosticsBatch {
    let diagnostics: Vec<Diagnostic> = (0..count)
        .map(|i| {
            Diagnostic::new(Severity::Warning, format!("d{i}"), (0, 0, 0, 1))
                .with_code(format!("E{i}"))
                .with_source("test")
        })
        .collect();
    DiagnosticsBatch::new(
        DocumentUri::new(uri),
        Generation::new(generation),
        diagnostics,
    )
}

#[tokio::test]
async fn empty_bus_publish_is_noop() {
    let bus = DiagnosticsBus::new();
    bus.publish(make_batch("file:///x", 1, 0)).await;
}

#[tokio::test]
async fn single_sink_receives_batch() {
    let bus = DiagnosticsBus::new();
    let recorder = Arc::new(Recorder {
        batches: Mutex::new(Vec::new()),
    });
    let count = bus.attach(Arc::clone(&recorder) as Arc<dyn DiagnosticsSink>);
    assert_eq!(count, 1);

    bus.publish(make_batch("file:///a", 2, 3)).await;
    let captured = recorder.batches.lock().expect("lock");
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].uri.as_str(), "file:///a");
    assert_eq!(captured[0].generation, Generation::new(2));
    assert_eq!(captured[0].diagnostics.len(), 3);
    assert_eq!(captured[0].diagnostics[0].code.as_deref(), Some("E0"));
    assert_eq!(captured[0].diagnostics[0].source.as_deref(), Some("test"));
}

#[tokio::test]
async fn multiple_sinks_each_receive_batch() {
    let bus = DiagnosticsBus::new();
    let a = Arc::new(Recorder {
        batches: Mutex::new(Vec::new()),
    });
    let b = Arc::new(Recorder {
        batches: Mutex::new(Vec::new()),
    });
    assert_eq!(bus.attach(Arc::clone(&a) as Arc<dyn DiagnosticsSink>), 1);
    assert_eq!(bus.attach(Arc::clone(&b) as Arc<dyn DiagnosticsSink>), 2);

    bus.publish(make_batch("file:///b", 5, 1)).await;
    assert_eq!(a.batches.lock().expect("lock").len(), 1);
    assert_eq!(b.batches.lock().expect("lock").len(), 1);
}

#[tokio::test]
async fn default_bus_works() {
    let bus = DiagnosticsBus::default();
    let r = Arc::new(Recorder {
        batches: Mutex::new(Vec::new()),
    });
    bus.attach(Arc::clone(&r) as Arc<dyn DiagnosticsSink>);
    bus.publish(make_batch("file:///c", 0, 0)).await;
    assert_eq!(r.batches.lock().expect("lock").len(), 1);
}

#[tokio::test]
async fn debug_format_shows_sink_count() {
    let bus = DiagnosticsBus::new();
    let r = Arc::new(Recorder {
        batches: Mutex::new(Vec::new()),
    });
    bus.attach(Arc::clone(&r) as Arc<dyn DiagnosticsSink>);
    let rendered = format!("{bus:?}");
    assert!(rendered.contains("DiagnosticsBus"));
    assert!(rendered.contains("sinks: 1"));
}

#[test]
fn document_uri_from_path_roundtrips_absolute() {
    #[cfg(not(windows))]
    let path = std::path::PathBuf::from("/tmp/diag.txt");
    #[cfg(windows)]
    let path = std::path::PathBuf::from("C:\\tmp\\diag.txt");
    let uri = document_uri_from_path(&path).expect("ok");
    assert!(uri.as_str().starts_with("file://"));
}

#[test]
fn document_uri_from_relative_path_errors() {
    let path = std::path::PathBuf::from("relative.txt");
    let err = document_uri_from_path(&path).expect_err("must reject");
    let rendered = format!("{err}");
    assert!(rendered.contains("file URI") || rendered.contains("path"));
}

#[test]
fn diagnostic_new_defaults_code_and_source_none() {
    let d = Diagnostic::new(Severity::Error, "boom", (1, 2, 3, 4));
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.message, "boom");
    assert_eq!(d.range, (1, 2, 3, 4));
    assert!(d.code.is_none());
    assert!(d.source.is_none());
}

#[test]
fn diagnostic_with_code_and_source_chain() {
    let d = Diagnostic::new(Severity::Hint, "hint", (0, 0, 0, 0))
        .with_code("L1")
        .with_source("clippy");
    assert_eq!(d.code.as_deref(), Some("L1"));
    assert_eq!(d.source.as_deref(), Some("clippy"));
}

#[test]
fn diagnostic_clone_works() {
    let d = Diagnostic::new(Severity::Information, "info", (5, 6, 7, 8));
    let copy = d.clone();
    assert_eq!(copy.severity, Severity::Information);
    assert_eq!(copy.range, (5, 6, 7, 8));
}

#[test]
fn severity_eq_and_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(Severity::Error);
    set.insert(Severity::Warning);
    set.insert(Severity::Information);
    set.insert(Severity::Hint);
    assert_eq!(set.len(), 4);
}

#[test]
fn batch_debug_works() {
    let b = make_batch("file:///x", 1, 1);
    assert!(format!("{b:?}").contains("DiagnosticsBatch"));
}

#[test]
fn batch_new_preserves_fields() {
    let b = make_batch("file:///z", 9, 2);
    assert_eq!(b.uri.as_str(), "file:///z");
    assert_eq!(b.generation, Generation::new(9));
    assert_eq!(b.diagnostics.len(), 2);
}
