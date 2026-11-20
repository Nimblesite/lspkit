//! Hover reply helpers. [LSP-HOVER-MARKDOWN]

use serde_json::{json, Value};

/// Build a `textDocument/hover` result carrying markdown contents.
///
/// `Some(value)` becomes `{ "contents": { "kind": "markdown", "value" } }`;
/// `None` becomes JSON `null`, the protocol's "no hover" result. The reply is
/// a plain [`Value`] so no protocol type leaks into a public signature.
/// [LSP-HOVER-MARKDOWN]
#[must_use]
pub fn hover_markdown(contents: Option<String>) -> Value {
    match contents {
        Some(value) => json!({ "contents": { "kind": "markdown", "value": value } }),
        None => Value::Null,
    }
}
