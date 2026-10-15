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

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lsp_profiler::wire::methods;
use lsp_profiler::{ProfilerConfig, ProfilerEvents, ProfilerService};
use lsp_profiler_dotnet::{DotNetConfig, DotNetRuntime};
use serde_json::{json, Value};

const FIXTURE_TYPE: &str = "HotLeakApp.LeakedPayload";
const HOT_FUNCTION: &str = "HotLeakApp.Hot.Spin";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn built(relative: &str, hint: &str) -> PathBuf {
    let path = repo_root().join(relative);
    assert!(
        path.is_file(),
        "{} is missing: run `{hint}` first",
        path.display()
    );
    path
}

/// The fixture app, killed on drop.
struct Fixture {
    child: Child,
    pid: u32,
}

impl Fixture {
    fn start() -> Self {
        let exe = built(
            "dotnet/fixtures/HotLeakApp/bin/Release/net8.0/HotLeakApp",
            "make dotnet",
        );
        let mut child = Command::new("./HotLeakApp")
            .current_dir(exe.parent().expect("fixture directory"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn HotLeakApp");
        let stdout = child.stdout.take().expect("fixture stdout");
        let mut line = String::new();
        let _ = BufReader::new(stdout)
            .read_line(&mut line)
            .expect("READY line");
        let pid: u32 = line
            .trim()
            .strip_prefix("READY ")
            .expect("READY <pid>")
            .parse()
            .expect("pid");
        Self { child, pid }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = stdin.write_all(b"\n");
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Default)]
struct Silent;

#[async_trait]
impl ProfilerEvents for Silent {
    async fn notify(&self, _method: &'static str, _params: Value) {}
}

fn service(output_dir: PathBuf) -> ProfilerService {
    let engine = built(
        "dotnet/LspProfiler.DotNet.Engine/bin/Release/net8.0/lsp-profiler-dotnet-engine",
        "make dotnet",
    );
    let runtime = DotNetRuntime::new(DotNetConfig {
        engine: Some(engine),
        output_dir: output_dir.clone(),
    });
    let config = ProfilerConfig {
        output_dir,
        ..ProfilerConfig::default()
    };
    ProfilerService::new(vec![Arc::new(runtime)], config, Arc::new(Silent))
}

async fn call(service: &ProfilerService, method: &str, params: Value) -> Value {
    service
        .handle(method, params)
        .await
        .unwrap_or_else(|error| panic!("{method} failed: {error}"))
}

fn completed(step: &Value) -> &Value {
    step.get("result")
        .or_else(|| step.pointer("/step/result"))
        .unwrap_or_else(|| panic!("direct backend must complete every step: {step}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_fixture_is_recognised_as_an_attachable_dotnet_process() {
    let fixture = Fixture::start();
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
    let fixture = Fixture::start();
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
    let fixture = Fixture::start();
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

/// A live process no runtime recognises (this test binary) is `UNSUPPORTED_RUNTIME`,
/// and kill refuses it — kill can never reach a process outside the profiler's reach.
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
        error_code(&service, methods::MEMORY_START, own).await,
        -32002
    );
    service.shutdown().await;
}

/// A target that exits mid-session keeps the samples taken while it lived: the
/// recorded trace is still symbolized and reported on stop ([PROF-CPU-STOP]).
#[tokio::test(flavor = "multi_thread")]
async fn a_target_that_exits_mid_session_keeps_its_samples() {
    let mut fixture = Fixture::start();
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
    let fixture = Fixture::start();
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
    let fixture = Fixture::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(
        &service,
        methods::CPU_START,
        json!({ "pid": fixture.pid, "duration_secs": 1.0 }),
    )
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
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
    let fixture = Fixture::start();
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
