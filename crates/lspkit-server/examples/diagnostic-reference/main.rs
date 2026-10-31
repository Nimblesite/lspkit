//! `diagnostic-reference` — real reference servers for the pull-diagnostic
//! stack, both built on one analysis core of real `serde_json` parses over
//! real files ([LSP-PULL-SALSA]).
//!
//! Default mode is an LSP server on framed stdio, driven end-to-end by the
//! crate's integration tests over native pipes: `Content-Length` framing,
//! `Dispatcher` scheduling with registration-before-scheduling cancellation
//! ([LSP-PULL-CANCELLATION]), `PullStore` memoization, `ReplyGate`-ordered
//! partial replies, and a native coalescing watcher that invalidates the
//! snapshot on disk changes ([LSP-PULL-WATCH]).
//!
//! `--backend` mode is a sidecar-framed out-of-process backend (4-byte
//! little-endian length-prefixed JSON over OS stdio) driven by
//! `lspkit`'s `tests/sidecar_inbound_routing.rs`.

mod analysis;
mod backend;
mod client;
mod client_wire;
mod ingress;
mod lifetime;
mod receive;
mod scan;
mod session;
mod watch;
mod wire;

/// Which reference server this process should run.
enum Mode {
    /// Framed-JSON backend for out-of-process IPC.
    Backend,
    /// Native IPC consumer rendering replies and unsolicited events.
    Client,
    /// Render a captured native framed stream from a real file.
    Receive,
    /// Render real coalesced native filesystem batches until the requested count.
    Watch(bool),
    /// Render workspace replacement and weak ingress ownership across natural release.
    Lifetime,
    /// The LSP session on stdio.
    Lsp,
}

/// The mode named by the first CLI argument, defaulting to the LSP session.
fn mode() -> Mode {
    match std::env::args().nth(1).as_deref() {
        Some("--backend") => Mode::Backend,
        Some("--client") => Mode::Client,
        Some("--receive") => Mode::Receive,
        Some("--watch") => Mode::Watch(true),
        Some("--watch-shallow") => Mode::Watch(false),
        Some("--lifetime") => Mode::Lifetime,
        _ => Mode::Lsp,
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    if let Err(error) = tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::WARN)
        .try_init()
    {
        return client::exit(Err(error), "tracing");
    }
    // The current-thread reference runtime keeps cancellation deterministic:
    // batched client frames are read in order before spawned work is polled.
    match mode() {
        Mode::Backend => backend::serve().await,
        Mode::Client => client::serve().await,
        Mode::Receive => receive::serve().await,
        Mode::Watch(recursive) => watch::serve(recursive).await,
        Mode::Lifetime => lifetime::serve().await,
        Mode::Lsp => session::serve().await,
    }
}
