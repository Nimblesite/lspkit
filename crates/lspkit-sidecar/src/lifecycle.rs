//! Sidecar lifecycle: spawn, health, restart.
//!
//! Wraps a [`tokio::process::Child`] with health-ping monitoring and
//! exponential-backoff respawn. The lifecycle manager is generic over the
//! request/response types; the transport layer handles framing.

use std::time::Duration;

use tokio::process::{Child, Command};

/// Lifecycle configuration.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct LifecycleConfig {
    /// Interval between health pings.
    pub ping_interval: Duration,
    /// Per-ping timeout.
    pub ping_timeout: Duration,
    /// Initial backoff after a crash.
    pub initial_backoff: Duration,
    /// Backoff ceiling.
    pub max_backoff: Duration,
    /// Maximum number of consecutive failed restarts before giving up.
    pub max_restarts: u32,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_secs(5),
            ping_timeout: Duration::from_secs(2),
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            max_restarts: u32::MAX,
        }
    }
}

/// Errors from the lifecycle manager.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    /// Spawning the child process failed.
    #[error("spawn failed: {0}")]
    Spawn(#[source] std::io::Error),
    /// Restart budget exhausted.
    #[error("restart budget exhausted after {0} attempts")]
    BudgetExhausted(u32),
}

/// Owns a single sidecar child process.
pub struct Sidecar {
    program: String,
    args: Vec<String>,
    child: Option<Child>,
    config: LifecycleConfig,
    restart_count: u32,
}

impl Sidecar {
    /// Construct, but do not yet spawn, a sidecar definition.
    #[must_use]
    pub fn new(program: impl Into<String>, args: Vec<String>, config: LifecycleConfig) -> Self {
        Self { program: program.into(), args, child: None, config, restart_count: 0 }
    }

    /// Spawn the child process.
    ///
    /// # Errors
    /// Returns [`LifecycleError::Spawn`] if the OS rejects the spawn.
    pub fn spawn(&mut self) -> Result<(), LifecycleError> {
        let child = Command::new(&self.program)
            .args(&self.args)
            .kill_on_drop(true)
            .spawn()
            .map_err(LifecycleError::Spawn)?;
        self.child = Some(child);
        Ok(())
    }

    /// Wait for the child to exit. Returns immediately if no child is running.
    pub async fn wait(&mut self) -> Option<std::process::ExitStatus> {
        if let Some(child) = self.child.as_mut() {
            child.wait().await.ok()
        } else {
            None
        }
    }

    /// Compute the next backoff duration using exponential growth capped at
    /// `max_backoff`.
    #[must_use]
    pub fn next_backoff(&self) -> Duration {
        let factor = 2_u32.saturating_pow(self.restart_count.min(20));
        let scaled = self.config.initial_backoff.checked_mul(factor).unwrap_or(self.config.max_backoff);
        scaled.min(self.config.max_backoff)
    }

    /// Increment the restart counter; returns the new count.
    pub fn record_restart(&mut self) -> u32 {
        self.restart_count = self.restart_count.saturating_add(1);
        self.restart_count
    }

    /// Reset the restart counter (e.g. after a successful health ping).
    pub fn reset_restart_count(&mut self) {
        self.restart_count = 0;
    }

    /// The configuration this sidecar was built with.
    #[must_use]
    pub fn config(&self) -> &LifecycleConfig {
        &self.config
    }
}

impl std::fmt::Debug for Sidecar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sidecar")
            .field("program", &self.program)
            .field("running", &self.child.is_some())
            .field("restarts", &self.restart_count)
            .finish_non_exhaustive()
    }
}
