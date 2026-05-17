//! Stdio-safe `tracing-subscriber` builder.
//!
//! Servers that speak JSON-RPC over stdio MUST keep their log output off
//! stdout. This builder writes to stderr by default and can additionally
//! emit JSON to a file.

use std::io;
use std::path::PathBuf;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Builder for a stdio-safe tracing subscriber.
#[non_exhaustive]
#[derive(Debug, Default)]
pub struct TracingBuilder {
    env_filter: Option<String>,
    json_file: Option<PathBuf>,
}

impl TracingBuilder {
    /// New builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the env-filter directive string.
    #[must_use]
    pub fn with_filter(mut self, directives: impl Into<String>) -> Self {
        self.env_filter = Some(directives.into());
        self
    }

    /// Also write JSON-formatted spans/events to `path`.
    #[must_use]
    pub fn with_json_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.json_file = Some(path.into());
        self
    }

    /// Install the subscriber as the global default.
    ///
    /// # Errors
    /// Returns an error if a subscriber has already been installed, the env
    /// filter is invalid, or the JSON file cannot be opened.
    pub fn install(self) -> Result<(), TracingError> {
        let env_filter = match self.env_filter {
            Some(directives) => {
                EnvFilter::try_new(directives).map_err(|e| TracingError::Filter(e.to_string()))?
            }
            None => EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        };

        let stderr_layer = fmt::layer().with_writer(io::stderr);
        let registry = tracing_subscriber::registry().with(env_filter).with(stderr_layer);

        if let Some(path) = self.json_file {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(TracingError::OpenFile)?;
            let json_layer = fmt::layer().json().with_writer(file);
            registry
                .with(json_layer)
                .try_init()
                .map_err(|e| TracingError::Init(e.to_string()))
        } else {
            registry.try_init().map_err(|e| TracingError::Init(e.to_string()))
        }
    }
}

/// Errors from installing a tracing subscriber.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum TracingError {
    /// Env-filter directives failed to parse.
    #[error("invalid filter directives: {0}")]
    Filter(String),
    /// JSON sink file could not be opened.
    #[error("could not open tracing JSON file: {0}")]
    OpenFile(#[source] io::Error),
    /// A subscriber was already installed, or initialization otherwise failed.
    #[error("could not install tracing subscriber: {0}")]
    Init(String),
}
