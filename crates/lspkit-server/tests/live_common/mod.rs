//! Panic-free JSON access for native watcher assertions. [LSP-PULL-WATCH]

use serde_json::Value;

/// Returns a null value for missing paths so the existing assertion reports the mismatch.
pub(crate) fn at<'a>(value: &'a Value, pointer: &str) -> &'a Value {
    value.pointer(pointer).unwrap_or(&Value::Null)
}
