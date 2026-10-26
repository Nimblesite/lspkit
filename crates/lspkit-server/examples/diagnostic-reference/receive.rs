//! Inspect a captured native framed stream through the actual shared reader.
//! [LSP-PULL-TRANSPORT]: errors are rendered explicitly, including initial reads.

use crate::client::{self, ClientError};
use crate::client_wire::{self, Closed};
use lspkit_sidecar::{inbound, DEFAULT_MAX_FRAME};
use std::process::ExitCode;

/// Open the caller's capture as a real native asynchronous file.
async fn input() -> Result<tokio::fs::File, ClientError> {
    let path = std::env::args_os().nth(2).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a framed capture path is required",
        )
    })?;
    Ok(tokio::fs::File::open(path).await?)
}

/// Observe real EOF or a terminal transport/codec error, then join ownership.
async fn observe() -> Result<(), ClientError> {
    let mut reader = inbound::spawn(
        input().await?,
        DEFAULT_MAX_FRAME,
        |bytes| client_wire::decode(&bytes),
        client_wire::is_notification,
    );
    let mut closed = Closed::default();
    let rendered = client_wire::render(&mut reader, &mut closed).await;
    if rendered.is_err() {
        reader.task.abort();
    }
    let joined = reader.task.await;
    client_wire::finish(closed, joined.is_ok(), None).await?;
    rendered?;
    joined?;
    Ok(())
}

/// Run the capture inspector; no backend process or success is fabricated.
pub(super) async fn serve() -> ExitCode {
    client::exit(observe().await, "framed_capture")
}
