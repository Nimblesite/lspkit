//! Native filesystem observations rendered by a real CLI. [LSP-PULL-WATCH]

use crate::client::{self, ClientError};
use crate::client_wire;
use lspkit_server::live::watcher::{ChangeEvent, CoalescedWatcher};
use lspkit_server::live::{FileWatcher, ScheduleConfig};
use serde_json::{json, Value};
use std::path::PathBuf;

/// A native OS path and optional batch count are normal CLI inputs.
fn options() -> Result<(PathBuf, usize), ClientError> {
    let mut args = std::env::args_os().skip(2);
    let root = args.next().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a native root path is required",
        )
    })?;
    let count = args
        .next()
        .map(|value| value.to_string_lossy().parse::<usize>())
        .transpose()?
        .unwrap_or(usize::MAX);
    Ok((PathBuf::from(root), count))
}

/// Serialize actual native event paths without replacing failed paths with blanks.
fn render_change(event: &ChangeEvent) -> Result<Value, ClientError> {
    let (kind, paths) = match event {
        ChangeEvent::Touched(paths) => ("touched", paths),
        ChangeEvent::Removed(paths) => ("removed", paths),
        ChangeEvent::Rescan => return Ok(json!({"kind":"rescan","paths":[]})),
        _ => return Err(std::io::Error::other("unsupported native change event").into()),
    };
    let paths = serde_json::to_value(paths).map_err(client_wire::WireError::from)?;
    Ok(json!({"kind":kind,"paths":paths}))
}

/// Readiness precedes filesystem interaction; release precedes natural process exit.
async fn run(root: PathBuf, count: usize, recursive: bool) -> Result<(), ClientError> {
    let native = if recursive {
        FileWatcher::new(&root)
    } else {
        FileWatcher::non_recursive(&root)
    }?;
    let mut watcher = native.coalesced(ScheduleConfig::default());
    client_wire::emit(&json!({"kind":"watching", "recursive":recursive})).await?;
    for _ in 0..count {
        emit_batch(&mut watcher).await?;
    }
    drop(watcher);
    client_wire::emit(&json!({"kind":"released"})).await?;
    Ok(())
}

/// Render one actual scheduler batch while preserving every native event.
async fn emit_batch(watcher: &mut CoalescedWatcher) -> Result<(), ClientError> {
    let batch = watcher
        .next()
        .await
        .ok_or_else(|| std::io::Error::other("native watcher stream closed"))?;
    let events = batch
        .events
        .iter()
        .map(render_change)
        .collect::<Result<Vec<_>, _>>()?;
    client_wire::emit(&json!({"kind":"batch","events":events})).await?;
    Ok(())
}

/// Native watching shares the same CLI failure boundary and output renderer.
pub(super) async fn serve(recursive: bool) -> std::process::ExitCode {
    let result = match options() {
        Ok((root, count)) => run(root, count, recursive).await,
        Err(error) => Err(error),
    };
    client::exit(result, "native_watcher")
}
