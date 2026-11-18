//! [PROF-DOTNET-MEMORY]: only the latest heap graph is ever read, so a session
//! polled for days must keep one graph on disk, not one per snapshot or diff.

use std::path::Path;

use super::{call, completed, methods, service, Fixture, FIXTURE, FIXTURE_TYPE};
use serde_json::json;

/// Graph-capturing steps, each superseding the graph before it.
const STEPS: [&str; 3] = [
    methods::MEMORY_SNAPSHOT,
    methods::MEMORY_DIFF,
    methods::MEMORY_SNAPSHOT,
];

/// Every heap graph file in the session's directory.
fn graphs(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .expect("the session's heap graph directory")
        .map(|entry| {
            entry
                .expect("a heap graph directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_snapshot_removes_the_graph_it_supersedes() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(
        &service,
        methods::MEMORY_START,
        json!({ "pid": fixture.pid }),
    )
    .await;
    let id = started["memory_session_id"].as_str().expect("a session id");
    let session = json!({ "memory_session_id": id });
    let heap = dir.path().join(format!("heap-{id}"));
    for step in STEPS {
        let answer = call(&service, step, session.clone()).await;
        assert!(
            completed(&answer)["outcome"].is_object(),
            "{step}: {answer}"
        );
        let kept = graphs(&heap);
        assert_eq!(
            kept.len(),
            1,
            "after {step} only the latest graph stays: {kept:?}"
        );
    }
    let refs =
        json!({ "memory_session_id": id, "type_name": FIXTURE_TYPE, "limit": 5, "max_depth": 3 });
    let answer = call(&service, methods::MEMORY_REFERENCES, refs).await;
    assert_eq!(
        completed(&answer)["outcome"]["kind"],
        "Refs",
        "the latest graph still answers: {answer}"
    );
    let _ = call(&service, methods::MEMORY_STOP, session).await;
    assert!(!heap.exists(), "stop removes the session's graphs");
    service.shutdown().await;
}
