//! Process recognition, CPU profiling and CPU session steps on the F# fixture.

use std::time::Duration;

use lsp_profiler::wire::methods;
use serde_json::{json, Value};

use crate::harness::{
    artifact, count, error_code, harness, in_file, in_program_fs, site, snapshot, start, stop,
    CLASSIFY_COUNT, ENTRY_POINT, FIXTURE, HOT_FUNCTION, MATCHING_FS, MATCH_FUNCTION, MATCH_LINES,
    ORDER_TYPE,
};
use crate::support::{assert_only_busy_threads_count, call, service, Fixture};

#[tokio::test(flavor = "multi_thread")]
async fn the_fsharp_fixture_is_recognised_as_an_attachable_dotnet_process() {
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
    assert_eq!(entry["runtime"], "dotnet", "{entry}");
    assert_eq!(entry["attachable"], true, "{entry}");
    assert!(
        entry["entry"].as_str().is_some_and(|e| e.contains(FIXTURE)),
        "the F# entry assembly is named: {entry}"
    );
    assert!(
        entry["runtime_version"]
            .as_str()
            .is_some_and(|v| !v.is_empty()),
        "the runtime version is resolved: {entry}"
    );
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cpu_profiling_finds_the_fsharp_hot_function_and_its_lines() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(&service, methods::CPU_START, json!({ "pid": fixture.pid })).await;
    assert_eq!(started["runtime"], "dotnet");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let report = call(
        &service,
        methods::CPU_STOP,
        json!({ "session_id": started["session_id"] }),
    )
    .await;
    assert!(
        report["total_samples"].as_i64().unwrap_or(0) > 100,
        "{report}"
    );
    let spin = report["hot_functions"]
        .as_array()
        .expect("hot_functions")
        .iter()
        .find(|f| {
            f["name"]
                .as_str()
                .is_some_and(|n| n.starts_with(HOT_FUNCTION))
        })
        .unwrap_or_else(|| panic!("{HOT_FUNCTION} absent: {report}"));
    assert!(in_program_fs(spin), "{spin}");
    assert!(
        spin["percentage"].as_f64().unwrap_or(0.0) >= 2.0,
        "[PROF-AGGREGATION-THRESHOLD] a hot function is at least 2%: {spin}"
    );
    assert!(
        spin["self_percentage"].as_f64().unwrap_or(0.0)
            <= spin["percentage"].as_f64().unwrap_or(0.0),
        "self never exceeds inclusive: {spin}"
    );
    assert!(
        report["hot_lines"]
            .as_array()
            .expect("hot_lines")
            .iter()
            .any(|l| in_program_fs(l) && (26..=31).contains(&l["line"].as_i64().unwrap_or(0))),
        "hot line inside Hot.spin expected: {report}"
    );
    assert_only_busy_threads_count(&report, ENTRY_POINT, HOT_FUNCTION);
    let shares: Vec<f64> = report["hot_functions"]
        .as_array()
        .expect("hot_functions")
        .iter()
        .map(|f| f["percentage"].as_f64().unwrap_or(0.0))
        .collect();
    assert!(
        shares.windows(2).all(|pair| pair[0] >= pair[1]),
        "[PROF-INVARIANTS] shares arrive ranked: {shares:?}"
    );
    for line in report["hot_lines"].as_array().expect("hot_lines") {
        assert!(
            line["line"].as_i64().unwrap_or(0) >= 1,
            "1-based line: {line}"
        );
        assert!(
            line["percentage"].as_f64().unwrap_or(0.0) >= 1.0,
            "[PROF-AGGREGATION-THRESHOLD] a line is hot at 1%: {line}"
        );
    }
    assert_every_export_names_the_fsharp_function(&report);
    service.shutdown().await;
}

/// [PROF-DOTNET-CPU-SYMBOLS]: F# compiles a match's decision tree to hidden
/// sequence points. A sample there still ran in `classify`, so it keeps the
/// method's file and the visible line before it: one hot function, in its file.
#[tokio::test(flavor = "multi_thread")]
async fn cpu_samples_in_fsharp_match_code_keep_their_file() {
    let mut fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(&service, methods::CPU_START, json!({ "pid": fixture.pid })).await;
    for _ in 0..3 {
        let classified = fixture.send(&format!("classify {CLASSIFY_COUNT}"));
        assert!(classified.starts_with("OK classify"), "{classified}");
    }
    let report = call(
        &service,
        methods::CPU_STOP,
        json!({ "session_id": started["session_id"] }),
    )
    .await;
    let entries: Vec<&Value> = report["hot_functions"]
        .as_array()
        .expect("hot_functions")
        .iter()
        .filter(|f| {
            f["name"]
                .as_str()
                .is_some_and(|n| n.starts_with(MATCH_FUNCTION))
        })
        .collect();
    assert!(
        entries.iter().all(|f| in_file(f, MATCHING_FS)),
        "every sample of the match keeps {MATCHING_FS}: {entries:?}"
    );
    assert_eq!(
        entries.len(),
        1,
        "{MATCH_FUNCTION} is one hot function, never split by a fileless twin: {report}"
    );
    let line = entries[0]["line"].as_i64().unwrap_or(0);
    assert!(
        MATCH_LINES.contains(&line),
        "its line is in its body: {line}"
    );
    let lines: Vec<i64> = report["hot_lines"]
        .as_array()
        .expect("hot_lines")
        .iter()
        .filter(|l| in_file(l, MATCHING_FS))
        .map(|l| l["line"].as_i64().unwrap_or(0))
        .collect();
    assert!(
        lines.iter().any(|l| MATCH_LINES.contains(l)),
        "a hot line sits inside the match: {lines:?}"
    );
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cpu_sessions_list_snapshot_and_refuse_a_second_attach() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(&service, methods::CPU_START, json!({ "pid": fixture.pid })).await;
    let session = json!({ "session_id": started["session_id"] });
    let listed = call(&service, methods::CPU_LIST, json!({})).await;
    let entry = listed["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|s| s["session_id"] == started["session_id"])
        .unwrap_or_else(|| panic!("the live session is listed: {listed}"));
    assert_eq!(entry["pid"], fixture.pid, "{entry}");
    assert_eq!(entry["runtime"], "dotnet", "{entry}");
    assert!(
        entry["started_at"].as_str().is_some_and(|t| !t.is_empty()),
        "a start time is reported: {entry}"
    );
    assert_eq!(
        error_code(&service, methods::CPU_START, json!({ "pid": fixture.pid })).await,
        -32004,
        "[PROF-CPU-START] a second session is ALREADY_PROFILING"
    );
    assert_eq!(
        listed["sessions"].as_array().map(Vec::len),
        Some(1),
        "the refusal stacks nothing"
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    let snapshot = call(&service, methods::CPU_SNAPSHOT, session.clone()).await;
    assert!(
        snapshot["total_samples"].as_i64().unwrap_or(0) > 0,
        "[PROF-CPU-SNAPSHOT] a mid-session snapshot has samples: {snapshot}"
    );
    let after = call(&service, methods::CPU_LIST, json!({})).await;
    assert!(
        after["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .any(|s| s["session_id"] == started["session_id"]),
        "[PROF-CPU-SESSIONS] a snapshot never ends the session: {after}"
    );
    let report = call(&service, methods::CPU_STOP, session.clone()).await;
    assert!(
        report["total_samples"].as_i64().unwrap_or(0) > 0,
        "{report}"
    );
    assert_eq!(
        error_code(&service, methods::CPU_STOP, session).await,
        -32000,
        "[PROF-ERRORS] stopping a stopped session is OTHER"
    );
    assert_eq!(
        call(&service, methods::CPU_LIST, json!({})).await["sessions"]
            .as_array()
            .map(Vec::len),
        Some(0),
        "no session survives the stop"
    );
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dead_pid_is_process_not_found_and_a_non_dotnet_pid_is_unsupported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    for method in [methods::CPU_START, methods::MEMORY_START] {
        assert_eq!(
            error_code(&service, method, json!({ "pid": 2_147_483_647 })).await,
            -32001,
            "[PROF-ERRORS] {method} on a missing PID"
        );
        assert_eq!(
            error_code(&service, method, json!({ "pid": std::process::id() })).await,
            -32002,
            "[PROF-ERRORS] {method} on a non-.NET process"
        );
    }
    assert_eq!(
        error_code(
            &service,
            methods::KILL,
            json!({ "pid": std::process::id() })
        )
        .await,
        -32002,
        "[PROF-PROCESSES-KILL] kill never reaches a process outside the profiler's reach"
    );
    service.shutdown().await;
}

/// [PROF-EXPORT]: each export is written and names the F# hot function.
fn assert_every_export_names_the_fsharp_function(report: &Value) {
    for key in ["speedscope_path", "flamegraph_path", "cpuprofile_path"] {
        let _ = artifact(report, key);
    }
    let speedscope: Value = serde_json::from_str(
        &std::fs::read_to_string(artifact(report, "speedscope_path")).unwrap(),
    )
    .expect("speedscope JSON");
    assert!(
        speedscope["shared"]["frames"]
            .as_array()
            .expect("frames")
            .iter()
            .any(|frame| frame["name"]
                .as_str()
                .is_some_and(|name| name.contains("Hot.spin"))),
        "a speedscope frame names the F# function: {speedscope}"
    );
    let cpuprofile: Value = serde_json::from_str(
        &std::fs::read_to_string(artifact(report, "cpuprofile_path")).unwrap(),
    )
    .expect("cpuprofile JSON");
    assert!(
        cpuprofile["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .any(|node| node["callFrame"]["functionName"]
                .as_str()
                .is_some_and(|name| name.contains("spin"))),
        "a .cpuprofile node names the F# function: {cpuprofile}"
    );
    let flamegraph = std::fs::read_to_string(artifact(report, "flamegraph_path")).unwrap();
    assert!(
        flamegraph.contains("<svg") && flamegraph.contains("spin"),
        "the flame graph shows the F# function"
    );
}

/// [PROF-DOTNET-HEAP-CAPTURE]: the runtime logs each type once, and forgets what
/// it logged only when no session asks for type events. A CPU session that asked
/// would keep that memory alive, so every heap snapshot after the first would
/// report the types it already named as bare addresses.
#[tokio::test(flavor = "multi_thread")]
async fn heap_snapshots_during_a_cpu_session_name_every_type() {
    let mut h = harness().await;
    let _ = h.fixture.send("work on");
    let cpu = call(
        &h.service,
        methods::CPU_START,
        json!({ "pid": h.fixture.pid }),
    )
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let session = start(&h).await;
    for orders in [100, 200] {
        let grown = h.fixture.send("grow 100");
        assert_eq!(count(&grown, "orders"), orders, "{grown}");
        let snapshot = snapshot(&h, &session).await;
        let names: Vec<&str> = snapshot["sites"]
            .as_array()
            .expect("sites")
            .iter()
            .filter_map(|site| site["type_name"].as_str())
            .collect();
        assert!(
            names.iter().all(|name| !name.starts_with("<type ")),
            "with {orders} orders, every type is named while the CPU session runs: {names:?}"
        );
        assert!(in_program_fs(site(&snapshot, ORDER_TYPE)), "{snapshot}");
    }
    let report = call(
        &h.service,
        methods::CPU_STOP,
        json!({ "session_id": cpu["session_id"] }),
    )
    .await;
    assert!(
        report["total_samples"].as_i64().unwrap_or(0) > 0,
        "{report}"
    );
    stop(&h, &session).await;
}
