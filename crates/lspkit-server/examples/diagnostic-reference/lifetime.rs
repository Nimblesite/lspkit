//! Render public ingress and workspace ownership through a real consumer. [LSP-PULL-CANCELLATION]
use std::path::PathBuf;
use std::sync::Arc;

use lspkit_server::workspace::{Workspace, WorkspaceFolder};
use lspkit_server::{CancellationHandle, Dispatcher, HandlerResult, JsonRpcError, RequestId};
use serde_json::{json, Value};

use crate::{client, client_wire};

/// The shared error type for this native consumer session.
type Outcome<T> = Result<T, Box<dyn std::error::Error>>;
/// The registered workspace method exercised by every scope snapshot.
const METHOD: &str = "reference/roots";

/// The consumer owns real native roots; the dispatcher handler owns this context.
struct Roots {
    /// The shared workspace whose folder ownership is observed.
    workspace: Workspace,
    /// The first native workspace root.
    first: PathBuf,
    /// The second native workspace root.
    second: PathBuf,
}

impl Roots {
    /// Claim the two CLI-named directories as workspace folders.
    fn from_args() -> Outcome<Self> {
        let [first, second] = options()?;
        let workspace = Workspace::new();
        workspace.set_folders(vec![WorkspaceFolder::new(first.clone(), "first")]);
        workspace.add(WorkspaceFolder::new(second.clone(), "second"));
        Ok(Self {
            workspace,
            first,
            second,
        })
    }

    /// Render the current workspace's folder count, names and ownership.
    fn report(&self) -> Value {
        let names: Vec<_> = self
            .workspace
            .folders()
            .into_iter()
            .map(|folder| folder.name)
            .collect();
        json!({
            "count":self.workspace.len(), "names":names,
            "first_owned":self.workspace.folder_for(&self.first).is_some(),
            "second_owned":self.workspace.folder_for(&self.second).is_some()
        })
    }
}

/// Native CLI arguments must name exactly two existing workspace directories.
fn options() -> Outcome<[PathBuf; 2]> {
    let paths: Vec<_> = std::env::args_os().skip(2).map(PathBuf::from).collect();
    let paths: [PathBuf; 2] = paths.try_into().map_err(|paths: Vec<PathBuf>| {
        std::io::Error::other(format!(
            "two workspace roots are required, got {}",
            paths.len()
        ))
    })?;
    if paths.iter().any(|path| !path.is_dir()) {
        return Err(std::io::Error::other("both workspace roots must exist").into());
    }
    Ok(paths)
}

/// The handler retains workspace context; a weak ingress observer must not retain this owner.
fn dispatcher(roots: Arc<Roots>) -> Dispatcher {
    let dispatcher = Dispatcher::new();
    dispatcher.register(METHOD, move |_, cancel| {
        let roots = Arc::clone(&roots);
        async move {
            if cancel.is_cancelled() {
                HandlerResult::Err(JsonRpcError::new(-32800, "Request cancelled"))
            } else {
                HandlerResult::Ok(roots.report())
            }
        }
    });
    dispatcher
}

/// Render the actual handler outcome without manufacturing success for an interrupted call.
fn render(result: HandlerResult) -> Outcome<Value> {
    match result {
        HandlerResult::Ok(result) => Ok(json!({"kind":"snapshot","result":result})),
        HandlerResult::Err(error) => Ok(json!({"kind":"failure","error":error})),
        _ => Err(std::io::Error::other("unsupported handler outcome").into()),
    }
}

/// A queued request's second registration must adopt its earlier client cancellation.
async fn interrupted(dispatcher: &Dispatcher, ingress: &CancellationHandle) -> Outcome<Value> {
    let id = RequestId::Number(3);
    ingress.register(id.clone(), None)?;
    ingress.cancel(&id);
    ingress.register(id.clone(), None)?;
    render(dispatcher.dispatch(&id, METHOD, Value::Null).await?)
}

/// Reader-side controls can arrive after their foreground owner has naturally exited.
fn late_inputs(ingress: &CancellationHandle) -> Outcome<()> {
    let id = RequestId::Number(4);
    ingress.register(id.clone(), None)?;
    ingress.cancel(&id);
    ingress.supersede(METHOD, "");
    ingress.supersede_all();
    Ok(())
}

/// Both scope snapshots use the same dispatcher and native output boundary.
async fn emit_scope(dispatcher: &Dispatcher, id: i64) -> Outcome<()> {
    let result = dispatcher
        .dispatch(&RequestId::Number(id), METHOD, Value::Null)
        .await?;
    client_wire::emit(&render(result)?).await?;
    Ok(())
}

/// Each interaction uses the same workspace and dispatcher until explicit owner release.
async fn run() -> Outcome<()> {
    let roots = Arc::new(Roots::from_args()?);
    let observed = Arc::downgrade(&roots);
    let dispatcher = dispatcher(Arc::clone(&roots));
    let ingress = dispatcher.cancellation_handle();
    emit_scope(&dispatcher, 1).await?;
    roots.workspace.remove(&roots.first);
    emit_scope(&dispatcher, 2).await?;
    client_wire::emit(&interrupted(&dispatcher, &ingress).await?).await?;
    drop(roots);
    drop(dispatcher);
    late_inputs(&ingress)?;
    client_wire::emit(&json!({"kind":"released","owner_released":observed.upgrade().is_none(),"late_inputs_accepted":true})).await?;
    Ok(())
}

/// All native consumer modes share the same structured failure boundary.
pub(super) async fn serve() -> std::process::ExitCode {
    client::exit(run().await, "native_lifetime")
}
