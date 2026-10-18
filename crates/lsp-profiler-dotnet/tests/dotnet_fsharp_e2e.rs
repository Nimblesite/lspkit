//! Full-stack .NET profiling of an F# program through the shared service. Tests
//! [PROF-DOTNET-CPU-SYMBOLS], [PROF-DOTNET-HEAP-GRAPH] and [PROF-DOTNET-SOURCE]
//! on the IL only F# emits.
//!
//! Real engine, real fixture, no mocks: `HotLeakAppFs` burns CPU in `Hot.spin`
//! and grows the module value `LeakCache.items` with records that hold union
//! cases and a generic record. Every member the compiler generates for a record or a union carries
//! only hidden sequence points, and a module value lives in a mangled
//! `items@<line>` field, so none of this is proven by the C# fixture.

#![allow(
    clippy::allow_attributes,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    missing_docs
)]

mod support;

use std::time::Duration;

use lsp_profiler::wire::methods;
use serde_json::{json, Value};
use support::{assert_only_busy_threads_count, call, completed, service, Fixture};

const FIXTURE: &str = "HotLeakAppFs";
const RECORD_TYPE: &str = "HotLeakAppFs.LeakedPayload";
const UNION_CASE_TYPE: &str = "HotLeakAppFs.PayloadKind+Small";
const GENERIC_RECORD_TYPE: &str = "HotLeakAppFs.Tagged`1[System.Int32]";
const HOT_FUNCTION: &str = "HotLeakAppFs.Hot.spin";
/// `Program.main`, which only ever waits on stdin.
const ENTRY_POINT: &str = "HotLeakAppFs.Program.main(";
/// `items.Add { Data = …; Label = …; Kind = kind }` in `LeakCache.grow`.
const RECORD_CONSTRUCTED_AT: i64 = 23;
/// `let kind = Small items.Count` in `LeakCache.grow`.
const UNION_CASE_CONSTRUCTED_AT: i64 = 21;
/// `let order = { Value = items.Count; Tag = "order" }` in `LeakCache.grow`.
const GENERIC_RECORD_CONSTRUCTED_AT: i64 = 22;
/// Types FSharp.Core defines, such as the printf machinery `$"..."` builds.
const FSHARP_CORE_NAMESPACE: &str = "Microsoft.FSharp.";

fn in_program_fs(value: &Value) -> bool {
    value["file"]
        .as_str()
        .is_some_and(|file| file.ends_with("Program.fs"))
}

fn site<'a>(snapshot: &'a Value, type_name: &str) -> &'a Value {
    snapshot["sites"]
        .as_array()
        .expect("sites")
        .iter()
        .find(|site| site["type_name"] == type_name)
        .unwrap_or_else(|| panic!("{type_name} absent: {snapshot}"))
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
        report["hot_lines"]
            .as_array()
            .expect("hot_lines")
            .iter()
            .any(|l| in_program_fs(l) && (26..=31).contains(&l["line"].as_i64().unwrap_or(0))),
        "hot line inside Hot.spin expected: {report}"
    );
    assert_only_busy_threads_count(&report, ENTRY_POINT, HOT_FUNCTION);
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fsharp_records_and_union_cases_anchor_where_the_program_constructs_them() {
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
    let step = call(&service, methods::MEMORY_SNAPSHOT, session.clone()).await;
    let snapshot = &completed(&step)["outcome"]["snapshot"];

    let record = site(snapshot, RECORD_TYPE);
    assert!(
        in_program_fs(record),
        "a record has a source location: {record}"
    );
    assert_eq!(record["line"], RECORD_CONSTRUCTED_AT, "{record}");
    let case = site(snapshot, UNION_CASE_TYPE);
    assert!(
        in_program_fs(case),
        "a union case has a source location: {case}"
    );
    assert_eq!(case["line"], UNION_CASE_CONSTRUCTED_AT, "{case}");
    let generic = site(snapshot, GENERIC_RECORD_TYPE);
    assert!(
        in_program_fs(generic),
        "a generic record has a source location: {generic}"
    );
    assert_eq!(generic["line"], GENERIC_RECORD_CONSTRUCTED_AT, "{generic}");

    tokio::time::sleep(Duration::from_secs(2)).await;
    let diff = call(&service, methods::MEMORY_DIFF, session.clone()).await;
    let outcome = &completed(&diff)["outcome"];
    let grown = outcome["diff"]["grown"]
        .as_array()
        .expect("grown")
        .iter()
        .find(|g| g["type_name"] == RECORD_TYPE)
        .unwrap_or_else(|| panic!("growth of {RECORD_TYPE} absent: {diff}"));
    assert!(grown["count_diff"].as_i64().unwrap_or(0) > 0, "{grown}");
    assert_eq!(
        grown["line"], RECORD_CONSTRUCTED_AT,
        "growth anchors in source: {grown}"
    );
    let _ = call(&service, methods::MEMORY_STOP, session).await;
    service.shutdown().await;
}

/// [PROF-DOTNET-SOURCE]: FSharp.Core embeds a PDB whose documents name the
/// build server's checkout. A location no editor on this machine can open is
/// no location: the type is reported by name alone, and the program's own
/// types keep their anchors.
#[tokio::test(flavor = "multi_thread")]
async fn a_pdb_naming_a_build_servers_paths_locates_nothing() {
    let mut fixture = Fixture::start(FIXTURE);
    // Orders hold an F# list, quotes a Set: FSharp.Core objects the program retains.
    let grown = fixture.send("grow 40");
    assert!(grown.contains(" orders=40 "), "{grown}");
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(
        &service,
        methods::MEMORY_START,
        json!({ "pid": fixture.pid }),
    )
    .await;
    let session = json!({ "memory_session_id": started["memory_session_id"] });
    let step = call(&service, methods::MEMORY_SNAPSHOT, session.clone()).await;
    let snapshot = &completed(&step)["outcome"]["snapshot"];
    let sites = snapshot["sites"].as_array().expect("sites");

    let library: Vec<&Value> = sites
        .iter()
        .filter(|s| {
            s["type_name"]
                .as_str()
                .is_some_and(|n| n.starts_with(FSHARP_CORE_NAMESPACE))
        })
        .collect();
    assert!(
        !library.is_empty(),
        "the program holds FSharp.Core objects: {snapshot}"
    );
    for site in &library {
        assert!(site["file"].is_null(), "no build-server anchor: {site}");
        assert!(site["line"].is_null(), "no line without a file: {site}");
    }
    for site in sites.iter().filter(|s| !s["file"].is_null()) {
        let file = site["file"].as_str().expect("file is a string");
        assert!(
            std::path::Path::new(file).exists(),
            "every anchor exists here: {site}"
        );
    }
    assert_eq!(site(snapshot, RECORD_TYPE)["line"], RECORD_CONSTRUCTED_AT);
    let _ = call(&service, methods::MEMORY_STOP, session).await;
    service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_fsharp_module_value_roots_the_leak_under_its_source_name() {
    let fixture = Fixture::start(FIXTURE);
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    let started = call(
        &service,
        methods::MEMORY_START,
        json!({ "pid": fixture.pid }),
    )
    .await;
    let id = &started["memory_session_id"];
    let _ = call(
        &service,
        methods::MEMORY_SNAPSHOT,
        json!({ "memory_session_id": id }),
    )
    .await;
    let refs = call(
        &service,
        methods::MEMORY_REFERENCES,
        json!({ "memory_session_id": id, "type_name": RECORD_TYPE, "limit": 40, "max_depth": 6 }),
    )
    .await;
    let graph = &completed(&refs)["outcome"]["graph"];
    let nodes = graph["nodes"].as_array().expect("nodes");
    assert!(
        nodes
            .iter()
            .any(|n| n["is_target"] == true && n["type_name"] == RECORD_TYPE),
        "{graph}"
    );
    let roots: Vec<&Value> = nodes.iter().filter(|n| n["is_root"] == true).collect();
    assert!(
        roots
            .iter()
            .any(|n| n["root_kind"] == "Static" && n["label"] == "static items"),
        "the module value `items` holds the leak: {roots:?}"
    );
    assert!(
        roots
            .iter()
            .all(|n| !n["label"].as_str().unwrap_or("").contains('@')),
        "no compiler-mangled field name reaches a label: {roots:?}"
    );
    let _ = call(
        &service,
        methods::MEMORY_STOP,
        json!({ "memory_session_id": id }),
    )
    .await;
    service.shutdown().await;
}
