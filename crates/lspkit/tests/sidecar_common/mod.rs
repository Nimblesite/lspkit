//! Panic-free JSON access for native wire assertions. [LSP-PULL-TRANSPORT]

use serde_json::Value;

/// Returns a null value for missing paths so the existing assertion reports the mismatch.
pub(crate) fn at<'a>(value: &'a Value, pointer: &str) -> &'a Value {
    value.pointer(pointer).unwrap_or(&Value::Null)
}

/// Returns a null value for missing entries so length and content assertions remain meaningful.
pub(crate) fn entry(values: &[Value], index: usize) -> &Value {
    values.get(index).unwrap_or(&Value::Null)
}
