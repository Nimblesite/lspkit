//! The F# fixture's constants and the helpers every F# suite module shares.

use std::path::PathBuf;

use lsp_profiler::wire::methods;
use lsp_profiler::ProfilerService;
use serde_json::{json, Value};

use crate::support::{call, completed, service, Fixture};

pub(crate) const FIXTURE: &str = "HotLeakAppFs";
pub(crate) const RECORD_TYPE: &str = "HotLeakAppFs.LeakedPayload";
pub(crate) const UNION_CASE_TYPE: &str = "HotLeakAppFs.PayloadKind+Small";
pub(crate) const GENERIC_RECORD_TYPE: &str = "HotLeakAppFs.Tagged`1[System.Int32]";
pub(crate) const HOT_FUNCTION: &str = "HotLeakAppFs.Hot.spin";
/// `Matching.classify`, whose active-pattern tests compile to hidden sequence points.
pub(crate) const MATCH_FUNCTION: &str = "HotLeakAppFs.Matching.classify(";
pub(crate) const MATCHING_FS: &str = "Matching.fs";
/// `classify` in Matching.fs, from its `let` to its last arm.
pub(crate) const MATCH_LINES: std::ops::RangeInclusive<i64> = 17..=22;
/// One `classify` command's numbers: about a second of matching per round.
pub(crate) const CLASSIFY_COUNT: u32 = 50_000_000;
/// `Program.main`, which only ever waits on stdin.
pub(crate) const ENTRY_POINT: &str = "HotLeakAppFs.Program.main(";
/// `items.Add { Data = …; Label = …; Kind = kind }` in `LeakCache.grow`.
pub(crate) const RECORD_CONSTRUCTED_AT: i64 = 23;
/// `let kind = Small items.Count` in `LeakCache.grow`.
pub(crate) const UNION_CASE_CONSTRUCTED_AT: i64 = 21;
/// `let order = { Value = items.Count; Tag = "order" }` in `LeakCache.grow`.
pub(crate) const GENERIC_RECORD_CONSTRUCTED_AT: i64 = 22;
/// The F# shapes the line protocol builds, each at the line that constructs it.
pub(crate) const ORDER_TYPE: &str = "HotLeakAppFs.Order";
pub(crate) const ORDER_LINE_TYPE: &str = "HotLeakAppFs.OrderLine";
pub(crate) const CUSTOMER_TYPE: &str = "HotLeakAppFs.CustomerId";
pub(crate) const EXPRESS_TYPE: &str = "HotLeakAppFs.Priority+Express";
pub(crate) const QUOTE_TYPE: &str = "HotLeakAppFs.Quote";
pub(crate) const NODE_TYPE: &str = "HotLeakAppFs.Node";

pub(crate) fn in_program_fs(value: &Value) -> bool {
    in_file(value, "Program.fs")
}

pub(crate) fn in_file(value: &Value, name: &str) -> bool {
    value["file"]
        .as_str()
        .is_some_and(|file| file.ends_with(name))
}

pub(crate) fn site<'a>(snapshot: &'a Value, type_name: &str) -> &'a Value {
    snapshot["sites"]
        .as_array()
        .expect("sites")
        .iter()
        .find(|site| site["type_name"] == type_name)
        .unwrap_or_else(|| panic!("{type_name} absent: {snapshot}"))
}

pub(crate) fn snapshot_of(step: &Value) -> &Value {
    &completed(step)["outcome"]["snapshot"]
}

/// The integer counts a fixture reply reports: `OK … orders=3 subscriptions=3 …`.
pub(crate) fn counts(reply: &str) -> std::collections::HashMap<&str, i64> {
    assert!(
        reply.starts_with("OK "),
        "the fixture accepted the command: {reply}"
    );
    reply
        .split_whitespace()
        .filter_map(|word| word.split_once('='))
        .filter_map(|(key, value)| value.parse().ok().map(|value| (key, value)))
        .collect()
}

pub(crate) fn count(reply: &str, key: &str) -> i64 {
    *counts(reply)
        .get(key)
        .unwrap_or_else(|| panic!("the reply carries {key}: {reply}"))
}

pub(crate) fn artifact(report: &Value, key: &str) -> PathBuf {
    let path = PathBuf::from(
        report["artifacts"][key]
            .as_str()
            .unwrap_or_else(|| panic!("{key} missing: {report}")),
    );
    assert!(path.is_file(), "{key} is written: {}", path.display());
    path
}

pub(crate) fn leak_type<'a>(diff: &'a Value, type_name: &str) -> &'a Value {
    diff["leaks"]
        .as_array()
        .expect("leaks")
        .iter()
        .find(|leak| leak["type_name"] == type_name)
        .unwrap_or_else(|| panic!("{type_name} is a suspected leak: {diff}"))
}

pub(crate) async fn error_code(service: &ProfilerService, method: &str, params: Value) -> i64 {
    match service.handle(method, params).await {
        Ok(value) => panic!("{method} must be refused, got {value}"),
        Err(error) => error.code(),
    }
}

/// A fresh F# program and service, with the workload stopped so every heap change
/// is one the test made. The `Fixture` must keep living for the test's duration.
pub(crate) struct Harness {
    pub(crate) fixture: Fixture,
    pub(crate) service: ProfilerService,
    #[allow(dead_code)]
    pub(crate) dir: tempfile::TempDir,
}

pub(crate) async fn harness() -> Harness {
    let mut fixture = Fixture::start(FIXTURE);
    let off = fixture.send("work off");
    assert_eq!(
        count(&off, "orders"),
        0,
        "a fresh program has no orders: {off}"
    );
    let release = fixture.send("release all");
    assert_eq!(count(&release, "payloads"), 0, "{release}");
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service(dir.path().to_path_buf());
    Harness {
        fixture,
        service,
        dir,
    }
}

pub(crate) async fn start(h: &Harness) -> Value {
    let started = call(
        &h.service,
        methods::MEMORY_START,
        json!({ "pid": h.fixture.pid }),
    )
    .await;
    assert_eq!(started["runtime"], "dotnet", "{started}");
    assert_eq!(started["mode"]["kind"], "Direct", "{started}");
    json!({ "memory_session_id": started["memory_session_id"] })
}

pub(crate) async fn snapshot(h: &Harness, session: &Value) -> Value {
    let step = call(&h.service, methods::MEMORY_SNAPSHOT, session.clone()).await;
    snapshot_of(&step).clone()
}

pub(crate) async fn diff(h: &Harness, session: &Value) -> Value {
    let step = call(&h.service, methods::MEMORY_DIFF, session.clone()).await;
    completed(&step)["outcome"].clone()
}

pub(crate) async fn stop(h: &Harness, session: &Value) {
    let stopped = call(&h.service, methods::MEMORY_STOP, session.clone()).await;
    assert_eq!(completed(&stopped)["outcome"]["kind"], "Ack", "{stopped}");
    h.service.shutdown().await;
}
