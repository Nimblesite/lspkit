//! Actual runtime negotiation and rejected requests must leave no active sessions.

use super::{call, methods, service, ProfilerService, Value};
use serde_json::json;

#[tokio::test]
async fn capability_handshake_and_rejected_requests_leave_no_sessions() {
    let directory = tempfile::tempdir().expect("profiler artifacts");
    let profiler = service(directory.path().to_path_buf());
    assert!(ProfilerService::handles(methods::CAPABILITIES));
    assert!(!ProfilerService::handles("initialize"));
    assert!(format!("{profiler:?}").contains("dotnet"));
    let capabilities = call(&profiler, methods::CAPABILITIES, Value::Null).await;
    assert_eq!(
        capabilities.pointer("/runtimes/0/id"),
        Some(&json!("dotnet"))
    );
    assert_eq!(
        capabilities.pointer("/runtimes/0/cpu/native_frames"),
        Some(&json!(true))
    );
    assert_eq!(
        capabilities.pointer("/runtimes/0/cpu/cooperative"),
        Some(&json!(false))
    );
    for params in [Value::Null, json!({"pid": -1}), json!({"pid": 0})] {
        let failure = profiler
            .handle(methods::CPU_START, params)
            .await
            .expect_err("invalid PID");
        assert_eq!(failure.code(), -32602);
    }
    let unsupported = profiler
        .handle(
            methods::CPU_COOPERATIVE_SCRIPT,
            json!({"runtime": "dotnet"}),
        )
        .await
        .expect_err("no cooperative sampler");
    assert!(unsupported.to_string().contains("no cooperative sampler"));
    let unknown = profiler
        .handle(
            methods::CPU_COOPERATIVE_ATTACH,
            json!({"runtime": "missing", "sample_file": "unused"}),
        )
        .await
        .expect_err("unknown runtime");
    assert!(unknown.to_string().contains("unknown runtime"));
    assert_eq!(
        call(&profiler, methods::CPU_LIST, Value::Null).await,
        json!({"sessions": []})
    );
    profiler.shutdown().await;
    assert_eq!(
        std::fs::read_dir(directory.path())
            .expect("artifacts")
            .count(),
        0
    );
}
