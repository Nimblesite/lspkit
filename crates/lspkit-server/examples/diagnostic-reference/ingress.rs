//! Consumer-owned input identities share the one registry. [LSP-PULL-CANCELLATION]
use lspkit_server::{CancellationHandle, DispatchError, Dispatcher, RequestId, RequestInterest};
use serde_json::Value;

/// Register before preparation; the returned future adopts this queued control.
pub(super) fn register(
    dispatcher: &Dispatcher,
    id: RequestId,
    method: &str,
    params: &Value,
) -> Result<(), DispatchError> {
    let handle = dispatcher.cancellation_handle();
    if method == "initialize" {
        handle.supersede_all();
    }
    if matches!(
        method,
        "initialize"
            | "shutdown"
            | "reference/work"
            | "textDocument/diagnostic"
            | "workspace/diagnostic"
    ) {
        handle.register(id, interest(method, params))?;
    }
    Ok(())
}

/// Opaque previous identities describe interest; they never become another result cache.
fn interest(method: &str, params: &Value) -> Option<RequestInterest> {
    let (resource, input) = match method {
        "textDocument/diagnostic" => (
            params.get("textDocument")?.get("uri")?.as_str()?,
            params
                .get("previousResultId")
                .and_then(Value::as_str)
                .map(str::to_owned),
        ),
        "workspace/diagnostic" => (
            "workspace",
            params.get("previousResultIds").map(Value::to_string),
        ),
        _ => return None,
    };
    Some(RequestInterest {
        method: method.to_owned(),
        resource: resource.to_owned(),
        input,
    })
}

/// Disk-input notifications retire matching pulls even when previous result IDs repeat.
pub(super) fn changed(handle: &CancellationHandle, params: &Value) {
    if let Some(changes) = params.get("changes").and_then(Value::as_array) {
        for uri in changes
            .iter()
            .filter_map(|change| change.get("uri").and_then(Value::as_str))
        {
            handle.supersede("textDocument/diagnostic", uri);
        }
        handle.supersede("workspace/diagnostic", "workspace");
    }
}
