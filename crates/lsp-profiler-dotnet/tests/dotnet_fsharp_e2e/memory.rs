//! What an F# heap snapshot reports: anchors, roots, the leak ladder, releases and package PDBs.

use lsp_profiler::wire::methods;
use serde_json::{json, Value};

use crate::harness::{
    count, diff, harness, in_program_fs, leak_type, site, snapshot, start, stop, CUSTOMER_TYPE,
    EXPRESS_TYPE, GENERIC_RECORD_CONSTRUCTED_AT, GENERIC_RECORD_TYPE, NODE_TYPE, ORDER_LINE_TYPE,
    ORDER_TYPE, QUOTE_TYPE, RECORD_CONSTRUCTED_AT, RECORD_TYPE, UNION_CASE_CONSTRUCTED_AT,
    UNION_CASE_TYPE,
};
use crate::support::{call, completed};

#[tokio::test(flavor = "multi_thread")]
async fn a_fsharp_records_and_unions_anchor_where_each_shape_is_constructed() {
    let mut h = harness().await;
    let _ = h.fixture.send("payload 100");
    let grown = h.fixture.send("grow 300");
    assert_eq!(count(&grown, "orders"), 300, "{grown}");
    assert_eq!(
        count(&grown, "subscriptions"),
        300,
        "one closure per order: {grown}"
    );
    assert_eq!(
        count(&grown, "cycles"),
        302,
        "a cycle plus one node per order: {grown}"
    );
    assert_eq!(
        count(&grown, "quotes"),
        300,
        "one cached quote per order: {grown}"
    );
    let started = call(
        &h.service,
        methods::MEMORY_START,
        json!({ "pid": h.fixture.pid, "max_sites": 200 }),
    )
    .await;
    let session = json!({ "memory_session_id": started["memory_session_id"] });
    let snapshot = snapshot(&h, &session).await;

    for (type_name, line, live) in [
        (RECORD_TYPE, RECORD_CONSTRUCTED_AT, 0),
        (UNION_CASE_TYPE, UNION_CASE_CONSTRUCTED_AT, 0),
        (GENERIC_RECORD_TYPE, GENERIC_RECORD_CONSTRUCTED_AT, 0),
        (ORDER_TYPE, 77, 300),
        (ORDER_LINE_TYPE, 75, 300),
        (CUSTOMER_TYPE, 73, 300),
        (EXPRESS_TYPE, 76, 100),
        (QUOTE_TYPE, 82, 300),
        (NODE_TYPE, 88, 302),
    ] {
        let found = site(&snapshot, type_name);
        assert!(
            in_program_fs(found),
            "a shape has a source location: {found}"
        );
        assert_eq!(
            found["line"], line,
            "{type_name} anchors where it is built: {found}"
        );
        assert!(found["size_bytes"].as_i64().unwrap_or(0) > 0, "{found}");
        assert!(found["count"].as_i64().unwrap_or(0) > 0, "{found}");
        if live > 0 {
            assert_eq!(found["count"], live, "{type_name}: every live instance");
        }
    }
    // The event-handler closure is an F# lambda, anchored at its own line.
    let closures = snapshot["sites"]
        .as_array()
        .expect("sites")
        .iter()
        .filter(|site| site["line"] == 97 && in_program_fs(site))
        .count();
    assert!(
        closures > 0,
        "the subscription closure anchors at line 97: {snapshot}"
    );

    // A struct record is inline and a nullary union case has no class, so neither is a heap type.
    let names: Vec<&str> = snapshot["sites"]
        .as_array()
        .expect("sites")
        .iter()
        .filter_map(|site| site["type_name"].as_str())
        .collect();
    assert!(
        !names.contains(&"HotLeakAppFs.Money"),
        "a struct record lives inline: {names:?}"
    );
    assert!(
        !names.contains(&"HotLeakAppFs.Priority+Standard"),
        "a nullary union case has no class: {names:?}"
    );

    // [PROF-DOTNET-SOURCE] arrays are the runtime's own type: never anchored to source.
    for array in [
        "System.Byte[]",
        "HotLeakAppFs.Order[]",
        "HotLeakAppFs.LeakedPayload[]",
    ] {
        let found = site(&snapshot, array);
        assert_eq!(
            found["file"],
            Value::Null,
            "{array} has no source location: {found}"
        );
        assert_eq!(found["line"], Value::Null, "{array} has no line: {found}");
    }
    stop(&h, &session).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_fsharp_leak_roots_are_module_values_under_their_source_names() {
    let mut h = harness().await;
    let _ = h.fixture.send("payload 100");
    let _ = h.fixture.send("grow 100");
    let session = start(&h).await;
    let _ = snapshot(&h, &session).await;

    for (type_name, expected_root) in [
        (RECORD_TYPE, "static items"),
        (ORDER_TYPE, "static orders"),
        (QUOTE_TYPE, "static cache"),
        (NODE_TYPE, "static cycles"),
    ] {
        let refs = call(
            &h.service,
            methods::MEMORY_REFERENCES,
            json!({
                "memory_session_id": session["memory_session_id"],
                "type_name": type_name,
                "limit": 60,
                "max_depth": 8
            }),
        )
        .await;
        let graph = &completed(&refs)["outcome"]["graph"];
        let nodes = graph["nodes"].as_array().expect("nodes");
        assert!(
            nodes
                .iter()
                .any(|n| n["is_target"] == true && n["type_name"] == type_name),
            "{type_name} instances are targets: {graph}"
        );
        let roots: Vec<&Value> = nodes.iter().filter(|n| n["is_root"] == true).collect();
        assert!(
            roots
                .iter()
                .any(|n| n["root_kind"] == "Static" && n["label"] == expected_root),
            "{type_name} is held by `{expected_root}`: {roots:?}"
        );
        assert!(
            roots
                .iter()
                .all(|n| !n["label"].as_str().unwrap_or("").contains('@')),
            "no compiler-mangled field name reaches a label: {roots:?}"
        );
        let edges = graph["edges"].as_array().expect("edges");
        assert!(!edges.is_empty(), "{type_name} has holding edges: {graph}");
        assert!(
            graph["cycles"].is_array(),
            "cycles are reported (possibly empty): {graph}"
        );
        assert!(graph["truncated"].is_boolean(), "{graph}");
        for node in nodes {
            assert!(node["depth"].as_i64().unwrap_or(-1) >= 0, "{node}");
            assert!(!node["label"].as_str().unwrap_or("").is_empty(), "{node}");
            assert!(
                !node["type_name"].as_str().unwrap_or("").is_empty(),
                "{node}"
            );
        }
    }
    stop(&h, &session).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_fsharp_leak_ladder_is_low_then_medium_then_high() {
    let mut h = harness().await;
    let _ = h.fixture.send("payload 100");
    let session = start(&h).await;
    let _ = snapshot(&h, &session).await;

    let mut bands = Vec::new();
    for _ in 0..3 {
        let _ = h.fixture.send("payload 100");
        let outcome = diff(&h, &session).await;
        let leak = leak_type(&outcome, RECORD_TYPE);
        assert!(leak["count_growth"].as_i64().unwrap_or(0) > 0, "{leak}");
        assert!(leak["size_growth"].as_i64().unwrap_or(0) > 0, "{leak}");
        assert!(
            !leak["reason"].as_str().unwrap_or("").is_empty(),
            "the ladder explains itself: {leak}"
        );
        assert!(in_program_fs(leak), "a leak anchors in source: {leak}");
        assert_eq!(leak["line"], RECORD_CONSTRUCTED_AT, "{leak}");
        bands.push(
            leak["confidence"]["kind"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        );
    }
    assert_eq!(
        bands,
        ["Low", "Medium", "High"],
        "[PROF-MEMORY-CONFIDENCE] {bands:?}"
    );
    stop(&h, &session).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn one_growth_over_ten_mebibytes_is_medium_and_arrays_stay_unlocated() {
    let mut h = harness().await;
    let _ = h.fixture.send("payload 10");
    let session = start(&h).await;
    let _ = snapshot(&h, &session).await;
    let added = h.fixture.send("blob 12");
    assert_eq!(count(&added, "blobs"), 12, "{added}");
    let outcome = diff(&h, &session).await;
    let leak = leak_type(&outcome, "System.Byte[]");
    assert!(
        leak["size_growth"].as_i64().unwrap_or(0) >= 10 * 1024 * 1024,
        "the 12 MiB growth is measured: {leak}"
    );
    assert_eq!(
        leak["confidence"]["kind"], "Medium",
        "[PROF-MEMORY-CONFIDENCE] one growth over 10 MiB is Medium: {leak}"
    );
    assert_eq!(
        leak["file"],
        Value::Null,
        "an array is never located: {leak}"
    );
    assert_eq!(leak["line"], Value::Null, "an array has no line: {leak}");
    stop(&h, &session).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_release_resets_the_streak_and_a_collection_frees_bytes() {
    let mut h = harness().await;
    let _ = h.fixture.send("payload 100");
    let session = start(&h).await;
    let _ = snapshot(&h, &session).await;
    let _ = h.fixture.send("payload 100");
    let low = diff(&h, &session).await;
    assert_eq!(leak_type(&low, RECORD_TYPE)["confidence"]["kind"], "Low");

    let released = h.fixture.send("release all");
    assert_eq!(count(&released, "payloads"), 0, "{released}");
    assert_eq!(count(&released, "orders"), 0, "{released}");
    let flat = diff(&h, &session).await;
    assert!(
        flat["leaks"]
            .as_array()
            .expect("leaks")
            .iter()
            .all(|leak| leak["type_name"] != RECORD_TYPE),
        "a released leak stops being suspected: {flat}"
    );
    assert!(
        flat["diff"]["total_freed"].as_i64().unwrap_or(0) >= 0,
        "freed is a magnitude: {flat}"
    );

    let gc = call(&h.service, methods::MEMORY_GC_COLLECT, session.clone()).await;
    let collected = &completed(&gc)["outcome"]["gc"];
    assert_eq!(
        collected["collected"], 1,
        "[PROF-DOTNET-GC] a collection is observed: {gc}"
    );
    assert!(
        collected["freed_bytes"].as_i64().unwrap_or(0) >= 0,
        "freed bytes are reported: {collected}"
    );
    assert_eq!(
        collected["uncollectable"].as_array().map(Vec::len),
        usize::try_from(collected["uncollectable_count"].as_u64().unwrap_or(0)).ok(),
        "{collected}"
    );

    let _ = h.fixture.send("payload 100");
    let again = diff(&h, &session).await;
    assert_eq!(
        leak_type(&again, RECORD_TYPE)["confidence"]["kind"],
        "Low",
        "[PROF-MEMORY-CONFIDENCE] a flat diff reset the streak: {again}"
    );
    stop(&h, &session).await;
}

/// [PROF-DOTNET-SOURCE]: FSharp.Core embeds a PDB whose documents name its build
/// server's checkout (`/Users/runner/work/…`). A location no editor here can open
/// is no location: lists, sets and handler closures are reported by name alone,
/// and the program's own types keep their anchors.
#[tokio::test(flavor = "multi_thread")]
async fn a_pdb_naming_a_build_servers_paths_locates_nothing() {
    let mut h = harness().await;
    let grown = h.fixture.send("grow 40");
    assert_eq!(count(&grown, "orders"), 40, "{grown}");
    let session = start(&h).await;
    let snapshot = snapshot(&h, &session).await;
    let sites = snapshot["sites"].as_array().expect("sites");

    let library: Vec<&Value> = sites
        .iter()
        .filter(|s| {
            s["type_name"]
                .as_str()
                .is_some_and(|n| n.starts_with("Microsoft.FSharp."))
        })
        .collect();
    assert!(
        !library.is_empty(),
        "the program holds FSharp.Core objects: {snapshot}"
    );
    for found in &library {
        assert_eq!(
            found["file"],
            Value::Null,
            "no build-server anchor: {found}"
        );
        assert_eq!(
            found["line"],
            Value::Null,
            "no line without a file: {found}"
        );
        assert!(
            found["count"].as_i64().unwrap_or(0) > 0,
            "still listed: {found}"
        );
    }
    for found in sites.iter().filter(|s| !s["file"].is_null()) {
        let file = found["file"].as_str().expect("file is a string");
        assert!(
            std::path::Path::new(file).exists(),
            "every anchor exists here: {found}"
        );
    }
    assert!(
        in_program_fs(site(&snapshot, ORDER_TYPE)),
        "the program's own record keeps its anchor"
    );
    stop(&h, &session).await;
}
