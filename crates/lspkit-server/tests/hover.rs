//! End-to-end tests for [`lspkit_server::hover_markdown`]. [LSP-HOVER-MARKDOWN]

use lspkit_server::hover_markdown;
use serde_json::json;

#[test]
fn some_wraps_markdown_in_contents_object() {
    let value = hover_markdown(Some("```rust\nlet x = 1;\n```".to_owned()));
    assert_eq!(
        value,
        json!({
            "contents": {
                "kind": "markdown",
                "value": "```rust\nlet x = 1;\n```"
            }
        })
    );
    let kind = value
        .get("contents")
        .and_then(|contents| contents.get("kind"))
        .and_then(|kind| kind.as_str());
    assert_eq!(kind, Some("markdown"));
}

#[test]
fn some_preserves_multiline_markdown_verbatim() {
    let markdown = "# Title\n\n- a\n- b\n\nemoji 😀 and CJK 数量\n";
    let value = hover_markdown(Some(markdown.to_owned()));
    let stored = value
        .get("contents")
        .and_then(|contents| contents.get("value"))
        .and_then(|value| value.as_str());
    assert_eq!(stored, Some(markdown));
}

#[test]
fn empty_string_still_emits_markdown_contents() {
    let value = hover_markdown(Some(String::new()));
    assert_eq!(
        value,
        json!({ "contents": { "kind": "markdown", "value": "" } })
    );
}

#[test]
fn none_is_json_null() {
    assert_eq!(hover_markdown(None), serde_json::Value::Null);
}
