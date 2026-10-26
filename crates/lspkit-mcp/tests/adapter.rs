//! Coverage for [`lspkit_mcp::Adapter`] construction, dispatch, and errors.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::{FakeEngine, FakeError};
use lspkit::EngineApi;
use lspkit_mcp::{Adapter, AdapterError, ToolDescription, ToolHandler};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

struct EchoHandler;

#[async_trait]
impl ToolHandler<FakeEngine> for EchoHandler {
    async fn invoke(&self, _engine: &FakeEngine, input: Value) -> Result<Value, AdapterError> {
        Ok(json!({ "echo": input }))
    }
}

struct ReportHandler;

#[async_trait]
impl ToolHandler<FakeEngine> for ReportHandler {
    async fn invoke(&self, engine: &FakeEngine, input: Value) -> Result<Value, AdapterError> {
        let query =
            input
                .get("q")
                .and_then(Value::as_str)
                .ok_or_else(|| AdapterError::InvalidInput {
                    tool: "report".into(),
                    message: "missing q".into(),
                })?;
        let snapshot = engine
            .report(query.to_owned(), CancellationToken::new())
            .await
            .map_err(|err| AdapterError::Engine(err.to_string()))?;
        Ok(json!({
            "generation": snapshot.generation.get(),
            "data": snapshot.data,
        }))
    }
}

struct ExplodingHandler;

#[async_trait]
impl ToolHandler<FakeEngine> for ExplodingHandler {
    async fn invoke(&self, _engine: &FakeEngine, _input: Value) -> Result<Value, AdapterError> {
        Err(AdapterError::Engine(
            FakeError::Boom("kaboom".into()).to_string(),
        ))
    }
}

fn description(name: &str) -> ToolDescription {
    ToolDescription::new(name, format!("desc {name}"), json!({ "type": "object" }))
}

#[tokio::test]
async fn new_adapter_has_no_tools() {
    let adapter = Adapter::new(FakeEngine::new());
    assert!(adapter.tools().is_empty());
}

#[tokio::test]
async fn register_adds_tool_description_to_snapshot() {
    let mut adapter = Adapter::new(FakeEngine::new());
    adapter.register(description("echo"), Arc::new(EchoHandler));
    let snapshot = adapter.tools();
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot.first().expect("one registered tool").name, "echo");
}

#[tokio::test]
async fn invoke_dispatches_to_registered_handler() {
    let mut adapter = Adapter::new(FakeEngine::new());
    adapter.register(description("echo"), Arc::new(EchoHandler));
    let result = adapter
        .invoke("echo", json!({ "v": 42 }))
        .await
        .expect("invoke ok");
    assert_eq!(result.name, "echo");
    assert_eq!(result.output, json!({ "echo": { "v": 42 } }));
}

#[tokio::test]
async fn invoke_unknown_tool_returns_error() {
    let adapter = Adapter::new(FakeEngine::new());
    let outcome = adapter.invoke("nope", json!({})).await;
    match outcome {
        Err(AdapterError::UnknownTool(name)) => assert_eq!(name, "nope"),
        other => assert!(
            matches!(other, Err(AdapterError::UnknownTool(_))),
            "{}",
            unexpected("unknown tool", &format!("{other:?}"))
        ),
    }
}

#[tokio::test]
async fn invoke_passes_engine_through_to_handler() {
    let mut adapter = Adapter::new(FakeEngine::new());
    adapter.register(description("report"), Arc::new(ReportHandler));
    let result = adapter
        .invoke("report", json!({ "q": "hello" }))
        .await
        .expect("invoke ok");
    assert_eq!(
        result.output.get("generation").and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        result.output.get("data").and_then(Value::as_str),
        Some("echo:hello")
    );
}

#[tokio::test]
async fn invoke_propagates_invalid_input_error_from_handler() {
    let mut adapter = Adapter::new(FakeEngine::new());
    adapter.register(description("report"), Arc::new(ReportHandler));
    let outcome = adapter.invoke("report", json!({})).await;
    match outcome {
        Err(AdapterError::InvalidInput { tool, message }) => {
            assert_eq!(tool, "report");
            assert!(message.contains("missing q"));
        }
        other => assert!(
            matches!(other, Err(AdapterError::InvalidInput { .. })),
            "{}",
            unexpected("invalid input", &format!("{other:?}"))
        ),
    }
}

#[tokio::test]
async fn invoke_propagates_engine_error_from_handler() {
    let mut adapter = Adapter::new(FakeEngine::new());
    adapter.register(description("boom"), Arc::new(ExplodingHandler));
    let outcome = adapter.invoke("boom", json!({})).await;
    match outcome {
        Err(AdapterError::Engine(msg)) => assert!(msg.contains("kaboom")),
        other => assert!(
            matches!(other, Err(AdapterError::Engine(_))),
            "{}",
            unexpected("engine error", &format!("{other:?}"))
        ),
    }
}

#[tokio::test]
async fn register_with_same_name_replaces_handler_and_description() {
    let mut adapter = Adapter::new(FakeEngine::new());
    adapter.register(description("echo"), Arc::new(EchoHandler));
    adapter.register(description("echo"), Arc::new(ExplodingHandler));
    assert_eq!(adapter.tools().len(), 1);
    let outcome = adapter.invoke("echo", json!({})).await;
    match outcome {
        Err(AdapterError::Engine(msg)) => assert!(msg.contains("kaboom")),
        other => assert!(
            matches!(other, Err(AdapterError::Engine(_))),
            "{}",
            unexpected("re-registered handler", &format!("{other:?}"))
        ),
    }
}

#[tokio::test]
async fn register_two_distinct_tools_preserves_insertion_order() {
    let mut adapter = Adapter::new(FakeEngine::new());
    adapter.register(description("first"), Arc::new(EchoHandler));
    adapter.register(description("second"), Arc::new(EchoHandler));
    let names: Vec<&str> = adapter.tools().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["first", "second"]);
}

#[tokio::test]
async fn adapter_clone_shares_engine_and_handlers() {
    let mut adapter = Adapter::new(FakeEngine::new());
    adapter.register(description("echo"), Arc::new(EchoHandler));
    let cloned = adapter.clone();
    let result = cloned
        .invoke("echo", json!({ "k": 1 }))
        .await
        .expect("clone can invoke");
    assert_eq!(result.output, json!({ "echo": { "k": 1 } }));
}

#[tokio::test]
async fn adapter_debug_includes_tool_count() {
    let mut adapter = Adapter::new(FakeEngine::new());
    adapter.register(description("echo"), Arc::new(EchoHandler));
    let rendered = format!("{adapter:?}");
    assert!(rendered.contains("Adapter"));
    assert!(rendered.contains("tools"));
    assert!(rendered.contains('1'));
}

#[tokio::test]
async fn adapter_error_display_messages_cover_all_variants() {
    let unknown = AdapterError::UnknownTool("x".into()).to_string();
    assert!(unknown.contains("tool not found"));
    let invalid = AdapterError::InvalidInput {
        tool: "y".into(),
        message: "bad".into(),
    }
    .to_string();
    assert!(invalid.contains("invalid input"));
    assert!(invalid.contains('y'));
    assert!(invalid.contains("bad"));
    let engine = AdapterError::Engine("nope".into()).to_string();
    assert!(engine.contains("engine error"));
    assert!(engine.contains("nope"));
}

#[tokio::test]
async fn tool_invocation_debug_includes_name() {
    let mut adapter = Adapter::new(FakeEngine::new());
    adapter.register(description("echo"), Arc::new(EchoHandler));
    let invocation = adapter.invoke("echo", json!({})).await.expect("invoke ok");
    let cloned = invocation.clone();
    assert_eq!(cloned.name, "echo");
    let rendered = format!("{invocation:?}");
    assert!(rendered.contains("echo"));
}

fn unexpected(expected: &str, actual: &str) -> String {
    format!("expected {expected}, got {actual}")
}
