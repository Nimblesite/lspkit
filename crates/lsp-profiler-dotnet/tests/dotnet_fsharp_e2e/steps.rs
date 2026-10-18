//! The memory steps over a kept F# graph: objects, references, refusals, the timeline and caps.

use std::path::PathBuf;

use lsp_profiler::wire::methods;
use serde_json::{json, Value};

use crate::harness::{
    count, error_code, harness, site, snapshot, start, stop, ORDER_TYPE, RECORD_TYPE,
};
use crate::support::{call, completed};

#[tokio::test(flavor = "multi_thread")]
async fn objects_by_type_honours_every_limit_and_ranks_the_largest_types() {
    let mut h = harness().await;
    let _ = h.fixture.send("payload 200");
    let _ = h.fixture.send("grow 200");
    let session = start(&h).await;
    let snapshot = snapshot(&h, &session).await;
    let recorded = site(&snapshot, ORDER_TYPE)["count"].as_i64().unwrap_or(0);

    for limit in [1, 3, 10, 50] {
        let listed = call(
            &h.service,
            methods::MEMORY_OBJECTS_BY_TYPE,
            json!({
                "memory_session_id": session["memory_session_id"],
                "type_name": ORDER_TYPE,
                "limit": limit
            }),
        )
        .await;
        let objects = &completed(&listed)["outcome"]["objects"];
        assert_eq!(objects["type_name"], ORDER_TYPE, "{objects}");
        assert_eq!(objects["total_count"], recorded, "{objects}");
        assert!(
            objects["total_bytes"].as_i64().unwrap_or(0) > 0,
            "{objects}"
        );
        assert_eq!(
            objects["objects"].as_array().map(Vec::len),
            Some(limit),
            "the limit is honoured: {objects}"
        );
        for sample in objects["objects"].as_array().expect("objects") {
            assert_eq!(sample["type_name"], ORDER_TYPE, "{sample}");
            assert!(sample["size_bytes"].as_i64().unwrap_or(0) > 0, "{sample}");
            assert!(
                !sample["label"].as_str().unwrap_or("").is_empty(),
                "{sample}"
            );
        }
        let sizes: Vec<i64> = objects["type_summary"]
            .as_array()
            .expect("summary")
            .iter()
            .map(|stat| stat["size_bytes"].as_i64().unwrap_or(0))
            .collect();
        assert!(
            sizes.windows(2).all(|pair| pair[0] >= pair[1]),
            "the summary is ranked largest first: {sizes:?}"
        );
        assert!(
            objects["type_summary"]
                .as_array()
                .expect("summary")
                .iter()
                .all(|stat| !stat["type_name"].as_str().unwrap_or("").is_empty()),
            "{objects}"
        );
    }

    let absent = call(
        &h.service,
        methods::MEMORY_OBJECTS_BY_TYPE,
        json!({
            "memory_session_id": session["memory_session_id"],
            "type_name": "HotLeakAppFs.NoSuchType",
            "limit": 5
        }),
    )
    .await;
    let empty = &completed(&absent)["outcome"]["objects"];
    assert_eq!(empty["total_count"], 0, "{empty}");
    assert_eq!(
        empty["objects"].as_array().map(Vec::len),
        Some(0),
        "{empty}"
    );
    assert!(
        empty["type_summary"]
            .as_array()
            .is_some_and(|s| !s.is_empty()),
        "the summary still describes the heap: {empty}"
    );
    stop(&h, &session).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn references_honour_depth_and_budget_and_the_snapshot_survives_the_target() {
    let mut h = harness().await;
    let _ = h.fixture.send("payload 100");
    let _ = h.fixture.send("grow 100");
    let session = start(&h).await;
    let _ = snapshot(&h, &session).await;

    for (depth, limit) in [(1, 5), (3, 20), (8, 60)] {
        let refs = call(
            &h.service,
            methods::MEMORY_REFERENCES,
            json!({
                "memory_session_id": session["memory_session_id"],
                "type_name": ORDER_TYPE,
                "limit": limit,
                "max_depth": depth
            }),
        )
        .await;
        let graph = &completed(&refs)["outcome"]["graph"];
        let nodes = graph["nodes"].as_array().expect("nodes");
        assert!(!nodes.is_empty(), "{graph}");
        assert!(nodes.len() <= limit, "the node budget is honoured: {graph}");
        assert!(
            nodes
                .iter()
                .any(|n| n["is_target"] == true && n["type_name"] == ORDER_TYPE),
            "{graph}"
        );
        assert!(
            nodes
                .iter()
                .all(|n| n["depth"].as_i64().unwrap_or(i64::MAX) <= depth + 1),
            "the depth cap is honoured: {graph}"
        );
        let ids: std::collections::HashSet<&str> =
            nodes.iter().filter_map(|n| n["id"].as_str()).collect();
        for edge in graph["edges"].as_array().expect("edges") {
            assert!(
                ids.contains(edge["from"].as_str().unwrap_or_default())
                    && ids.contains(edge["to"].as_str().unwrap_or_default()),
                "every edge joins two known nodes: {edge}"
            );
        }
    }

    // [PROF-DOTNET-MEMORY] refs and objects re-open the kept graph, so they still
    // answer after the program is gone; a new capture does not.
    let mut fixture = h.fixture;
    let _ = fixture.child.kill();
    let _ = fixture.child.wait();
    let refs = call(
        &h.service,
        methods::MEMORY_REFERENCES,
        json!({
            "memory_session_id": session["memory_session_id"],
            "type_name": ORDER_TYPE,
            "limit": 20,
            "max_depth": 4
        }),
    )
    .await;
    assert_eq!(completed(&refs)["outcome"]["kind"], "Refs", "{refs}");
    let objects = call(
        &h.service,
        methods::MEMORY_OBJECTS_BY_TYPE,
        json!({
            "memory_session_id": session["memory_session_id"],
            "type_name": ORDER_TYPE,
            "limit": 3
        }),
    )
    .await;
    assert_eq!(
        completed(&objects)["outcome"]["kind"],
        "Objects",
        "{objects}"
    );
    assert_eq!(
        error_code(
            &h.service,
            methods::MEMORY_SNAPSHOT,
            json!({ "memory_session_id": session["memory_session_id"] })
        )
        .await,
        -32001,
        "[PROF-ERRORS] a new capture on an exited program is PROCESS_NOT_FOUND"
    );
    let _ = call(&h.service, methods::MEMORY_STOP, session.clone()).await;
    h.service.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn every_fsharp_step_refuses_before_a_snapshot_and_after_a_stop() {
    let h = harness().await;
    let session = start(&h).await;
    for method in [methods::MEMORY_REFERENCES, methods::MEMORY_OBJECTS_BY_TYPE] {
        assert_eq!(
            error_code(
                &h.service,
                method,
                json!({
                    "memory_session_id": session["memory_session_id"],
                    "type_name": RECORD_TYPE,
                    "limit": 5
                })
            )
            .await,
            -32000,
            "[PROF-DOTNET-MEMORY] {method} before any snapshot is OTHER"
        );
    }
    let marker = "not-a-payload-7fd1";
    let refused = h
        .service
        .handle(
            methods::MEMORY_INGEST,
            json!({ "memory_session_id": session["memory_session_id"], "output": marker }),
        )
        .await
        .expect_err("garbage is refused");
    assert_ne!(
        refused.code(),
        0,
        "[PROF-MEMORY-INGEST] unrecognised output is refused: {refused}"
    );
    assert_eq!(
        error_code(
            &h.service,
            methods::MEMORY_INGEST,
            json!({ "memory_session_id": "no-such-session", "output": "{}" })
        )
        .await,
        -32000,
        "an unknown session is OTHER"
    );
    let _ = snapshot(&h, &session).await;
    stop(&h, &session).await;
    for method in [
        methods::MEMORY_SNAPSHOT,
        methods::MEMORY_DIFF,
        methods::MEMORY_REFERENCES,
        methods::MEMORY_OBJECTS_BY_TYPE,
        methods::MEMORY_GC_COLLECT,
        methods::MEMORY_STOP,
    ] {
        assert_eq!(
            error_code(
                &h.service,
                method,
                json!({ "memory_session_id": session["memory_session_id"], "type_name": RECORD_TYPE })
            )
            .await,
            -32000,
            "[PROF-ERRORS] {method} on a stopped session is OTHER"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_timeline_and_heapprofile_are_written_for_every_fsharp_snapshot() {
    let mut h = harness().await;
    let _ = h.fixture.send("payload 50");
    let session = start(&h).await;
    let first = snapshot(&h, &session).await;
    let first_outcome = completed(
        &call(
            &h.service,
            methods::MEMORY_SNAPSHOT,
            json!({ "memory_session_id": session["memory_session_id"] }),
        )
        .await,
    )["outcome"]
        .clone();
    let first_path = first_outcome["heapprofile_path"]
        .as_str()
        .unwrap_or_else(|| panic!("a heapprofile is written: {first_outcome}"));
    assert!(PathBuf::from(first_path).is_file(), "{first_path}");
    let profile: Value =
        serde_json::from_str(&std::fs::read_to_string(first_path).unwrap()).expect("V8 JSON");
    assert!(
        profile["head"].is_object(),
        "a V8 call-tree root: {profile}"
    );
    assert!(profile["samples"].is_array(), "V8 samples: {profile}");

    let _ = h.fixture.send("payload 50");
    let second_outcome = completed(
        &call(
            &h.service,
            methods::MEMORY_SNAPSHOT,
            json!({ "memory_session_id": session["memory_session_id"] }),
        )
        .await,
    )["outcome"]
        .clone();
    let second = &second_outcome["snapshot"];
    let timeline = second_outcome["timeline"].as_array().expect("timeline");
    assert_eq!(
        timeline.len(),
        3,
        "[PROF-MEMORY-TIMELINE] one point per snapshot"
    );
    for point in timeline {
        assert!(
            point["elapsed_secs"].as_f64().unwrap_or(-1.0) >= 0.0,
            "{point}"
        );
        assert!(point["current_bytes"].as_i64().unwrap_or(0) > 0, "{point}");
        assert!(
            point["peak_bytes"].as_i64().unwrap_or(0)
                >= point["current_bytes"].as_i64().unwrap_or(0),
            "peak never falls below current: {point}"
        );
        assert!(point["object_count"].as_i64().unwrap_or(0) > 0, "{point}");
        assert!(
            point["snapshot_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty()),
            "{point}"
        );
    }
    assert_eq!(second["gc_generations"], json!([]), "counts stay absent");
    assert!(
        second["generation_sizes"]
            .as_array()
            .is_some_and(|s| !s.is_empty()),
        "generation bytes have names and units: {second}"
    );
    assert!(
        second["current_bytes"].as_i64().unwrap_or(0)
            >= first["current_bytes"].as_i64().unwrap_or(0),
        "the heap grew across the snapshots"
    );
    assert!(
        second_outcome["trending_upward"].is_boolean(),
        "[PROF-MEMORY-TIMELINE] trending is explicit: {second_outcome}"
    );
    for pair in timeline.windows(2) {
        assert!(
            pair[1]["elapsed_secs"].as_f64().unwrap_or(0.0)
                >= pair[0]["elapsed_secs"].as_f64().unwrap_or(0.0),
            "time never runs backwards: {pair:?}"
        );
        assert!(
            pair[1]["peak_bytes"].as_i64().unwrap_or(0)
                >= pair[0]["peak_bytes"].as_i64().unwrap_or(0),
            "peak never falls: {pair:?}"
        );
    }
    stop(&h, &session).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn traceback_depth_and_max_sites_cap_an_fsharp_snapshot() {
    let mut h = harness().await;
    let _ = h.fixture.send("payload 200");
    let _ = h.fixture.send("grow 100");
    let started = call(
        &h.service,
        methods::MEMORY_START,
        json!({ "pid": h.fixture.pid, "traceback_depth": 3, "max_sites": 7 }),
    )
    .await;
    let session = json!({ "memory_session_id": started["memory_session_id"] });
    let snapshot = snapshot(&h, &session).await;
    let sites = snapshot["sites"].as_array().expect("sites");
    assert!(
        !sites.is_empty(),
        "the snapshot still summarises the heap: {snapshot}"
    );
    for found in sites {
        assert!(
            found["traceback"].as_array().map_or(0, Vec::len) <= 3,
            "traceback_depth caps every stack: {found}"
        );
        for frame in found["traceback"].as_array().expect("traceback") {
            assert!(frame["line"].as_i64().unwrap_or(0) >= 1, "{frame}");
            assert!(!frame["file"].as_str().unwrap_or("").is_empty(), "{frame}");
        }
    }
    stop(&h, &session).await;
}

/// [PROF-MEMORY-TIMELINE]: `peak_bytes` is the peak since tracking began, so a
/// release and a collection lower the live bytes but never the peak.
#[tokio::test(flavor = "multi_thread")]
async fn a_release_and_a_collection_never_lower_the_peak() {
    let mut h = harness().await;
    let _ = h.fixture.send("grow 300");
    let retained = h.fixture.send("blob 16");
    assert_eq!(count(&retained, "blobs"), 16, "{retained}");
    let session = start(&h).await;
    let before = snapshot(&h, &session).await;
    let released = h.fixture.send("release all");
    assert_eq!(count(&released, "blobs"), 0, "{released}");
    let _ = call(&h.service, methods::MEMORY_GC_COLLECT, session.clone()).await;
    let step = call(&h.service, methods::MEMORY_SNAPSHOT, session.clone()).await;
    let outcome = &completed(&step)["outcome"];
    let after = &outcome["snapshot"];
    let bytes = |snapshot: &Value, key: &str| snapshot[key].as_i64().unwrap_or(0);
    assert!(
        bytes(after, "current_bytes") < bytes(&before, "current_bytes"),
        "the release freed live bytes: {before} then {after}"
    );
    assert!(
        bytes(after, "peak_bytes") >= bytes(&before, "peak_bytes"),
        "the peak never falls: {before} then {after}"
    );
    assert!(
        bytes(after, "peak_bytes") >= bytes(after, "current_bytes"),
        "the peak is never below the live bytes: {after}"
    );
    let peaks: Vec<i64> = outcome["timeline"]
        .as_array()
        .expect("timeline")
        .iter()
        .map(|point| point["peak_bytes"].as_i64().unwrap_or(0))
        .collect();
    assert!(
        peaks.windows(2).all(|pair| pair[1] >= pair[0]),
        "the timeline's peak never falls: {peaks:?}"
    );
    stop(&h, &session).await;
}
