//! Coverage for `ToolDescription` and `ToolRegistry`.

#![cfg(feature = "mcp")]

use lspkit_server::mcp::{ToolDescription, ToolRegistry};
use serde_json::json;

fn make_tool(name: &str) -> ToolDescription {
    ToolDescription::new(
        name,
        format!("desc for {name}"),
        json!({ "type": "object" }),
    )
}

#[test]
fn registry_starts_empty_and_lookup_misses() {
    let registry = ToolRegistry::new();
    assert!(registry.list().is_empty());
    assert!(registry.get("missing").is_none());
}

#[test]
fn registry_default_matches_new() {
    let registry = ToolRegistry::default();
    assert!(registry.list().is_empty());
}

#[test]
fn registry_registers_and_lists_in_order() {
    let mut registry = ToolRegistry::new();
    registry.register(make_tool("alpha"));
    registry.register(make_tool("beta"));
    let listed: Vec<&str> = registry.list().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(listed, vec!["alpha", "beta"]);
}

#[test]
fn registry_get_returns_registered_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(make_tool("alpha"));
    let found = registry.get("alpha").expect("should find alpha");
    assert_eq!(found.name, "alpha");
    assert_eq!(found.description, "desc for alpha");
    assert_eq!(found.input_schema, json!({ "type": "object" }));
}

#[test]
fn registry_replaces_tool_with_same_name_in_place() {
    let mut registry = ToolRegistry::new();
    registry.register(make_tool("alpha"));
    registry.register(make_tool("beta"));
    let replacement = ToolDescription::new("alpha", "new alpha", json!({ "k": 1 }));
    registry.register(replacement);
    assert_eq!(registry.list().len(), 2);
    let alpha = registry.get("alpha").expect("alpha still there");
    assert_eq!(alpha.description, "new alpha");
    assert_eq!(alpha.input_schema, json!({ "k": 1 }));
    let names: Vec<&str> = registry.list().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta"]);
}

#[test]
fn tool_description_clone_and_debug() {
    let tool = make_tool("gamma");
    let cloned = tool.clone();
    assert_eq!(cloned.name, tool.name);
    let rendered = format!("{tool:?}");
    assert!(rendered.contains("gamma"));
}

#[test]
fn tool_registry_clone_preserves_entries() {
    let mut registry = ToolRegistry::new();
    registry.register(make_tool("alpha"));
    let cloned = registry.clone();
    let rendered = format!("{cloned:?}");
    assert!(rendered.contains("alpha"));
    assert_eq!(cloned.list().len(), 1);
}
