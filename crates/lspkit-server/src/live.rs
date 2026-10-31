//! `lspkit_server::live` — file-watcher, debouncer, and session lifecycle.
//!
//! Produces values that an [`lspkit::EngineApi`] implementation can build on:
//! a coarse change stream from [`watcher::FileWatcher`], coalesced by
//! [`scheduler::spawn`], driving an [`session::Analyzer`] that advances a
//! [`session::Session`]'s generation counter on each successful pass.

pub mod scheduler;
pub mod session;
pub mod watcher;

pub use crate::live::scheduler::{spawn as spawn_scheduler, ScheduleConfig, Trigger};
pub use crate::live::session::{Analyzer, AnalyzerError, AnalyzerResult, Session};
pub use crate::live::watcher::{ChangeEvent, FileWatcher, WatcherError};
