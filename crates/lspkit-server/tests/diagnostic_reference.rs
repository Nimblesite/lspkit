//! Native CLI sessions prove the public pull-diagnostic consumer contract.
//! [LSP-PULL-CAPABILITIES] [LSP-PULL-SALSA] [LSP-PULL-CANCELLATION] [LSP-PULL-WATCH]
#![cfg(feature = "pull")]

mod common;

#[path = "cli_sessions/capabilities.rs"]
mod capabilities;
#[path = "cli_sessions/dispatcher.rs"]
mod dispatcher;
#[path = "cli_sessions/interest.rs"]
mod interest;
#[path = "cli_sessions/pulls.rs"]
mod pulls;
#[path = "cli_sessions/workspace.rs"]
mod workspace;

#[path = "cli_sessions/blocked.rs"]
mod blocked;

#[cfg(unix)]
#[path = "cli_sessions/unrepresentable.rs"]
mod unrepresentable;
