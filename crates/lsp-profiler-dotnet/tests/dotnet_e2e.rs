//! Full-stack .NET profiling through the shared service. Tests [PROF-DOTNET],
//! [PROF-DOTNET-CPU], [PROF-DOTNET-MEMORY] and [PROF-DOTNET-GC].
//!
//! Real engine, real fixture, no mocks: `HotLeakApp` burns CPU in `Hot.Spin`
//! and grows `LeakCache.Items` until stdin closes. The service is driven over
//! its wire methods exactly as a language server host would drive it. The
//! engine and the fixture must be built first (`make dotnet`); their absence
//! fails the suite rather than skipping it.

#![allow(
    clippy::allow_attributes,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    missing_docs
)]

mod support;

use std::path::PathBuf;
use std::time::Duration;

use lsp_profiler::wire::methods;
use lsp_profiler::ProfilerService;
use serde_json::{json, Value};
use support::{assert_only_busy_threads_count, call, completed, engine, service, Fixture};

const FIXTURE: &str = "HotLeakApp";
const FIXTURE_TYPE: &str = "HotLeakApp.LeakedPayload";
const HOT_FUNCTION: &str = "HotLeakApp.Hot.Spin";
/// `Program.Main`, which only ever waits on stdin.
const ENTRY_POINT: &str = "HotLeakApp.Program.Main(";
/// `LeakCache.Items`: one small object, outside any top-twenty summary.
const LEAK_LIST: &str = "System.Collections.Generic.List`1[HotLeakApp.LeakedPayload]";

#[tokio::test(flavor = "multi_thread")]
async fn the_fixture_is_recognised_as_an_attachable_dotnet_process() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let list = call(
        &service,
        methods::PROCESSES,
        json!({ "workspace_roots": [] }),
    )
    .await;
    let entry = list["processes"]
        .as_array()
        .expect("processes")
        .iter()
        .find(|p| p["pid"] == fixture.pid)
        .cloned()
        .unwrap_or_else(|| panic!("fixture pid {} missing from {list}", fixture.pid));
    assert_eq!(entry["runtime"], "dotnet");
    assert_eq!(entry["attachable"], true, "{entry}");
    assert!(
        entry["entry"]
            .as_str()
            .is_some_and(|e| e.contains("HotLeakApp")),
        "{entry}"
    );
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cpu_profiling_finds_the_hot_function_and_its_lines() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(&service, methods::CPU_START, json!({ "pid": fixture.pid })).await;
    let session_id = started["session_id"]
        .as_str()
        .expect("session_id")
        .to_owned();
    assert_eq!(started["runtime"], "dotnet");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let report = call(
        &service,
        methods::CPU_STOP,
        json!({ "session_id": session_id }),
    )
    .await;
    assert!(
        report["total_samples"].as_i64().unwrap_or(0) > 100,
        "{report}"
    );
    let functions = report["hot_functions"].as_array().expect("hot_functions");
    let spin = functions
        .iter()
        .find(|f| {
            f["name"]
                .as_str()
                .is_some_and(|n| n.starts_with(HOT_FUNCTION))
        })
        .unwrap_or_else(|| panic!("{HOT_FUNCTION} absent: {report}"));
    assert!(
        spin["file"]
            .as_str()
            .is_some_and(|f| f.ends_with("Program.cs")),
        "{spin}"
    );
    let lines = report["hot_lines"].as_array().expect("hot_lines");
    assert!(
        lines.iter().any(|l| l["file"]
            .as_str()
            .is_some_and(|f| f.ends_with("Program.cs"))
            && (29..=32).contains(&l["line"].as_i64().unwrap_or(0))),
        "hot line inside Hot.Spin expected: {report}"
    );
    assert_only_busy_threads_count(&report, ENTRY_POINT, HOT_FUNCTION);
    for key in ["speedscope_path", "flamegraph_path", "cpuprofile_path"] {
        let path = report["artifacts"][key]
            .as_str()
            .unwrap_or_else(|| panic!("{key} missing: {report}"));
        assert!(PathBuf::from(path).is_file(), "{key} not written: {path}");
    }
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn memory_profiling_sees_the_leak_its_root_and_the_collection() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(
        &service,
        methods::MEMORY_START,
        json!({ "pid": fixture.pid }),
    )
    .await;
    assert_eq!(started["mode"]["kind"], "Direct", "{started}");
    let session = json!({ "memory_session_id": started["memory_session_id"] });

    let first = call(&service, methods::MEMORY_SNAPSHOT, session.clone()).await;
    let snapshot = &completed(&first)["outcome"]["snapshot"];
    let site = snapshot["sites"]
        .as_array()
        .expect("sites")
        .iter()
        .find(|s| s["type_name"] == FIXTURE_TYPE)
        .cloned()
        .unwrap_or_else(|| panic!("{FIXTURE_TYPE} absent: {first}"));
    assert!(
        site["file"]
            .as_str()
            .is_some_and(|f| f.ends_with("Program.cs")),
        "{site}"
    );
    assert_eq!(site["line"], 7, "{site}");
    assert_eq!(
        snapshot["gc_generations"],
        json!([]),
        "unreported collection counts must stay absent"
    );
    assert_eq!(
        snapshot["generation_sizes"].as_array().map(Vec::len),
        Some(5),
        "generation bytes must have an explicit unit, not be presented as collection counts"
    );
    assert_eq!(snapshot["generation_sizes"][0]["name"], "Gen 0");
    assert_eq!(snapshot["generation_sizes"][3]["name"], "LOH");
    assert_eq!(snapshot["generation_sizes"][4]["name"], "POH");
    assert!(
        snapshot["generation_sizes"]
            .as_array()
            .expect("generation sizes")
            .iter()
            .map(|row| row["size_bytes"].as_i64().expect("bytes"))
            .sum::<i64>()
            > 0
    );

    tokio::time::sleep(Duration::from_secs(2)).await;
    let diff = call(&service, methods::MEMORY_DIFF, session.clone()).await;
    let grown = completed(&diff)["outcome"]["diff"]["grown"]
        .as_array()
        .expect("grown")
        .iter()
        .find(|g| g["type_name"] == FIXTURE_TYPE)
        .cloned()
        .unwrap_or_else(|| panic!("growth of {FIXTURE_TYPE} absent: {diff}"));
    assert!(grown["count_diff"].as_i64().unwrap_or(0) > 0, "{grown}");

    let refs = call(&service, methods::MEMORY_REFERENCES, json!({ "memory_session_id": started["memory_session_id"], "type_name": FIXTURE_TYPE, "limit": 40, "max_depth": 6 })).await;
    let graph = &completed(&refs)["outcome"]["graph"];
    let nodes = graph["nodes"].as_array().expect("nodes");
    assert!(
        nodes
            .iter()
            .any(|n| n["is_target"] == true && n["type_name"] == FIXTURE_TYPE),
        "{graph}"
    );
    assert!(
        nodes.iter().any(|n| n["is_root"] == true
            && n["root_kind"] == "Static"
            && n["label"] == "static Items"),
        "static root Items expected: {graph}"
    );
    assert!(
        !graph["edges"].as_array().expect("edges").is_empty(),
        "{graph}"
    );

    let objects = call(&service, methods::MEMORY_OBJECTS_BY_TYPE, json!({ "memory_session_id": started["memory_session_id"], "type_name": FIXTURE_TYPE, "limit": 3 })).await;
    let list = &completed(&objects)["outcome"]["objects"];
    assert!(list["total_count"].as_i64().unwrap_or(0) > 0, "{list}");
    assert_eq!(list["objects"].as_array().map(Vec::len), Some(3), "{list}");

    let gc = call(&service, methods::MEMORY_GC_COLLECT, session.clone()).await;
    assert_eq!(completed(&gc)["outcome"]["gc"]["collected"], 1, "{gc}");

    let stopped = call(&service, methods::MEMORY_STOP, session).await;
    assert_eq!(completed(&stopped)["outcome"]["kind"], "Ack", "{stopped}");
    service.shutdown().await;
}

async fn error_code(service: &ProfilerService, method: &str, params: Value) -> i64 {
    match service.handle(method, params).await {
        Ok(value) => panic!("{method} must be refused, got {value}"),
        Err(error) => error.code(),
    }
}

/// A PID that names no process is `PROCESS_NOT_FOUND` on every PID-taking method —
/// never "no runtime recognises it", which is the answer for a live process of
/// another kind ([PROF-ERRORS], [PROF-PROCESSES-KILL]).
#[tokio::test(flavor = "multi_thread")]
async fn a_pid_that_names_no_process_is_process_not_found_everywhere() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let missing = json!({ "pid": 999_999 });
    assert_eq!(
        error_code(&service, methods::KILL, missing.clone()).await,
        -32001
    );
    assert_eq!(
        error_code(&service, methods::CPU_START, missing.clone()).await,
        -32001
    );
    assert_eq!(
        error_code(&service, methods::MEMORY_START, missing).await,
        -32001
    );
    service.shutdown().await;
}

/// A live process no runtime recognises (this test binary) is `UNSUPPORTED_RUNTIME`
/// for every PID-taking command, answered well under the engine's connect timeout:
/// the engine decides "no .NET diagnostics endpoint" up front ([PROF-ERRORS],
/// [PROF-DOTNET-ENGINE-ENDPOINT]). Kill refuses it — kill can never reach a
/// process outside the profiler's reach.
#[tokio::test(flavor = "multi_thread")]
async fn a_live_process_no_runtime_recognises_is_unsupported_and_never_killed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let own = json!({ "pid": std::process::id() });
    assert_eq!(
        error_code(&service, methods::KILL, own.clone()).await,
        -32002
    );
    assert_eq!(
        error_code(&service, methods::MEMORY_START, own.clone()).await,
        -32002
    );
    let started = tokio::time::Instant::now();
    assert_eq!(error_code(&service, methods::CPU_START, own).await, -32002);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the engine answers without waiting for a connect timeout: {:?}",
        started.elapsed()
    );
    service.shutdown().await;
}

/// The engine itself refuses a live process with no .NET diagnostics endpoint as
/// `UnsupportedRuntime` immediately, before any connection attempt
/// ([PROF-DOTNET-ENGINE-ENDPOINT]). Drives the engine binary directly so the
/// endpoint decision is proven at the engine boundary, not through the service.
#[tokio::test(flavor = "multi_thread")]
async fn the_engine_refuses_a_process_without_a_dotnet_endpoint_immediately() {
    let started = std::time::Instant::now();
    let output = std::process::Command::new(engine())
        .args(["cpu", "--pid", &std::process::id().to_string()])
        .output()
        .expect("run the engine");
    let elapsed = started.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last: Value = serde_json::from_str(stdout.lines().last().unwrap_or_default())
        .unwrap_or_else(|error| panic!("one protocol line expected ({error}): {stdout}"));
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert_eq!(last["kind"], "failure", "{last}");
    assert_eq!(last["error_kind"], "UnsupportedRuntime", "{last}");
    assert!(
        elapsed < Duration::from_secs(5),
        "the engine answers without waiting for a connect timeout: {elapsed:?}"
    );
}

/// `cpu.start` resolves the runtime before anything else ([PROF-CPU-START]
/// step 1): naming a runtime that cannot profile the PID is
/// `UNSUPPORTED_RUNTIME` even while the PID has a CPU session, and only a
/// start that could attach is `ALREADY_PROFILING`.
#[tokio::test(flavor = "multi_thread")]
async fn a_named_runtime_is_resolved_before_the_pid_is_found_busy() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(&service, methods::CPU_START, json!({ "pid": fixture.pid })).await;
    let named = json!({ "pid": fixture.pid, "runtime": "python" });
    assert_eq!(
        error_code(&service, methods::CPU_START, named).await,
        -32002
    );
    let again = json!({ "pid": fixture.pid });
    assert_eq!(
        error_code(&service, methods::CPU_START, again).await,
        -32004
    );
    let _ = call(
        &service,
        methods::CPU_STOP,
        json!({ "session_id": started["session_id"] }),
    )
    .await;
    service.shutdown().await;
}

/// A memory step on a program that has exited is `PROCESS_NOT_FOUND`, the
/// engine's classified cause, never a bare `OTHER` ([PROF-ERRORS]).
#[tokio::test(flavor = "multi_thread")]
async fn a_memory_step_on_an_exited_program_is_process_not_found() {
    let mut fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(
        &service,
        methods::MEMORY_START,
        json!({ "pid": fixture.pid }),
    )
    .await;
    let session = json!({ "memory_session_id": started["memory_session_id"] });
    let _ = fixture.child.kill();
    let _ = fixture.child.wait();
    assert_eq!(
        error_code(&service, methods::MEMORY_SNAPSHOT, session).await,
        -32001
    );
    service.shutdown().await;
}

/// A target that exits mid-session keeps the samples taken while it lived: the
/// recorded trace is still symbolized and reported on stop ([PROF-CPU-STOP]).
#[tokio::test(flavor = "multi_thread")]
async fn a_target_that_exits_mid_session_keeps_its_samples() {
    let mut fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(&service, methods::CPU_START, json!({ "pid": fixture.pid })).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let _ = fixture.child.kill();
    let _ = fixture.child.wait();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let report = call(
        &service,
        methods::CPU_STOP,
        json!({ "session_id": started["session_id"] }),
    )
    .await;
    assert!(
        report["total_samples"].as_i64().unwrap_or(0) > 0,
        "samples taken before the exit are reported: {report}"
    );
    service.shutdown().await;
}

/// `cpu.start` reports the same runtime version the process listing resolved,
/// never an empty string ([PROF-CPU-START]).
#[tokio::test(flavor = "multi_thread")]
async fn cpu_start_reports_the_runtime_version_the_listing_knows() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let list = call(
        &service,
        methods::PROCESSES,
        json!({ "workspace_roots": [] }),
    )
    .await;
    let listed = list["processes"]
        .as_array()
        .and_then(|all| all.iter().find(|p| p["pid"] == fixture.pid))
        .map(|p| p["runtime_version"].clone())
        .unwrap_or_default();
    assert!(listed.as_str().is_some_and(|v| !v.is_empty()), "{list}");
    let started = call(&service, methods::CPU_START, json!({ "pid": fixture.pid })).await;
    assert_eq!(started["runtime_version"], listed, "{started}");
    let _ = call(
        &service,
        methods::CPU_STOP,
        json!({ "session_id": started["session_id"] }),
    )
    .await;
    service.shutdown().await;
}

/// A duration ends the session by itself: the kept report records the
/// requested time, not the wait until `stop` ([PROF-CPU-SESSIONS]).
#[tokio::test(flavor = "multi_thread")]
async fn a_duration_caps_the_recorded_time() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(
        &service,
        methods::CPU_START,
        json!({ "pid": fixture.pid, "duration_secs": 1.0 }),
    )
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    // [PROF-CPU-SESSIONS] the duration ended the session: it is no longer active.
    let listed = call(&service, methods::CPU_LIST, json!({})).await;
    assert!(
        !listed["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .any(|session| session["session_id"] == started["session_id"]),
        "a session past its duration is not listed as active: {listed}"
    );
    // Its report is kept for the next stop.
    let report = call(
        &service,
        methods::CPU_STOP,
        json!({ "session_id": started["session_id"] }),
    )
    .await;
    let secs = report["duration_secs"].as_f64().unwrap_or(0.0);
    assert!(
        (0.9..=1.1).contains(&secs),
        "recorded the 1 s requested: {report}"
    );
    assert!(
        report["total_samples"].as_i64().unwrap_or(0) > 0,
        "{report}"
    );
    service.shutdown().await;
}

/// `cpu.snapshot` reports the data so far without ending the session
/// ([PROF-CPU-SNAPSHOT]). The .NET engine only emits ticks on stop today, so a
/// mid-session snapshot has nothing to report: this test pins that defect.
#[tokio::test(flavor = "multi_thread")]
async fn a_mid_session_snapshot_reports_the_samples_so_far() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(&service, methods::CPU_START, json!({ "pid": fixture.pid })).await;
    let session = json!({ "session_id": started["session_id"] });
    tokio::time::sleep(Duration::from_secs(2)).await;
    let snapshot = call(&service, methods::CPU_SNAPSHOT, session.clone()).await;
    let _ = call(&service, methods::CPU_STOP, session).await;
    assert!(
        snapshot["total_samples"].as_i64().unwrap_or(0) > 0,
        "two seconds in, the snapshot has samples: {snapshot}"
    );
    service.shutdown().await;
}

/// A heap the capture buffer cannot hold is refused, naming the dropped events,
/// and never summarised as a smaller heap: lost edges cut live objects off from
/// their roots ([PROF-DOTNET-HEAP-CAPTURE]). A 1 MB buffer cannot hold the
/// burst of a heap that has grown for two seconds.
#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_that_loses_events_fails_instead_of_misreporting_the_heap() {
    let fixture = Fixture::start(FIXTURE);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let graph = dir.path().join("snapshot.heapgraph.json");
    let output = std::process::Command::new(engine())
        .arg("snapshot")
        .args(["--pid", &fixture.pid.to_string()])
        .arg("--graph")
        .arg(&graph)
        .args(["--buffer-mb", "1"])
        .output()
        .expect("run the engine");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last: Value = serde_json::from_str(stdout.lines().last().unwrap_or_default())
        .unwrap_or_else(|error| panic!("one protocol line expected ({error}): {stdout}"));
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert_eq!(last["kind"], "failure", "{last}");
    assert_eq!(last["error_kind"], "Failed", "{last}");
    let message = last["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("dropped") && message.contains("1 MB capture buffer"),
        "the failure says why: {message}"
    );
    assert!(!graph.exists(), "no partial graph is kept");
}

/// Objects of a type too small to rank among the largest types still get their
/// own summary row, agreeing with the totals ([PROF-DOTNET-MEMORY] objects):
/// the one `List<LeakedPayload>` that holds the leak is 32 bytes, far below the
/// twenty largest types.
#[tokio::test(flavor = "multi_thread")]
async fn objects_of_a_small_type_keep_its_own_summary_row() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(
        &service,
        methods::MEMORY_START,
        json!({ "pid": fixture.pid }),
    )
    .await;
    let session = json!({ "memory_session_id": started["memory_session_id"] });
    let snapshot = call(&service, methods::MEMORY_SNAPSHOT, session.clone()).await;
    assert_eq!(
        completed(&snapshot)["outcome"]["kind"],
        "Snapshot",
        "{snapshot}"
    );
    let params = json!({ "memory_session_id": started["memory_session_id"], "type_name": LEAK_LIST, "limit": 1 });
    let objects = call(&service, methods::MEMORY_OBJECTS_BY_TYPE, params).await;
    let list = &completed(&objects)["outcome"]["objects"];
    let rows = list["type_summary"].as_array().expect("type summary");
    let own: Vec<&Value> = rows
        .iter()
        .filter(|row| row["type_name"] == LEAK_LIST)
        .collect();
    assert_eq!(own.len(), 1, "one row for the requested type: {list}");
    assert_eq!(
        list["total_count"], 1,
        "LeakCache.Items is the only list of payloads: {list}"
    );
    assert_eq!(own[0]["count"], list["total_count"], "{list}");
    assert_eq!(own[0]["size_bytes"], list["total_bytes"], "{list}");
    let sizes: Vec<i64> = rows
        .iter()
        .map(|row| row["size_bytes"].as_i64().expect("bytes"))
        .collect();
    assert!(
        sizes.windows(2).all(|pair| pair[0] >= pair[1]),
        "still largest first: {sizes:?}"
    );
    let _ = call(&service, methods::MEMORY_STOP, session).await;
    service.shutdown().await;
}
