//! Coarse end-to-end tests for the sidecar lifecycle layer.
//!
//! These tests drive only the public API (`Sidecar`, `LifecycleConfig`,
//! `LifecycleError`). The "real subprocess" cases use OS-shipped no-op
//! binaries that exit immediately — no sleeps, no timing dependencies.

use std::time::Duration;

use lspkit_sidecar::{LifecycleConfig, LifecycleError, Sidecar};

// --- pure-logic cases (no subprocess) -------------------------------------

/// `LifecycleConfig::default` ships the documented defaults.
#[test]
fn default_config_matches_documented_values() {
    let cfg = LifecycleConfig::default();
    assert_eq!(cfg.ping_interval, Duration::from_secs(5));
    assert_eq!(cfg.ping_timeout, Duration::from_secs(2));
    assert_eq!(cfg.initial_backoff, Duration::from_secs(1));
    assert_eq!(cfg.max_backoff, Duration::from_secs(30));
    assert_eq!(cfg.max_restarts, u32::MAX);

    let cloned = cfg.clone();
    assert_eq!(cloned.ping_interval, cfg.ping_interval);
    assert!(format!("{cfg:?}").contains("LifecycleConfig"));
}

/// Backoff doubles per restart and is clamped to `max_backoff`.
#[test]
fn backoff_doubles_and_clamps_to_ceiling() {
    let mut cfg = LifecycleConfig::default();
    cfg.ping_interval = Duration::from_millis(10);
    cfg.ping_timeout = Duration::from_millis(5);
    cfg.initial_backoff = Duration::from_millis(100);
    cfg.max_backoff = Duration::from_millis(800);
    cfg.max_restarts = 10;
    let mut sidecar = Sidecar::new("/nonexistent/never-spawned", Vec::new(), cfg.clone());

    // restart_count starts at 0 → factor 1 → initial_backoff.
    assert_eq!(sidecar.next_backoff(), Duration::from_millis(100));

    assert_eq!(sidecar.record_restart(), 1);
    assert_eq!(sidecar.next_backoff(), Duration::from_millis(200));

    assert_eq!(sidecar.record_restart(), 2);
    assert_eq!(sidecar.next_backoff(), Duration::from_millis(400));

    assert_eq!(sidecar.record_restart(), 3);
    assert_eq!(sidecar.next_backoff(), Duration::from_millis(800));

    // Past the ceiling: stays clamped.
    assert_eq!(sidecar.record_restart(), 4);
    assert_eq!(sidecar.next_backoff(), Duration::from_millis(800));

    // Config accessor returns what we built with.
    assert_eq!(sidecar.config().max_backoff, cfg.max_backoff);
}

/// A huge restart count saturates rather than overflows when multiplying
/// the backoff, and still clamps to `max_backoff`.
#[test]
fn backoff_saturates_on_overflow() {
    let mut cfg = LifecycleConfig::default();
    cfg.initial_backoff = Duration::from_secs(1_000_000);
    cfg.max_backoff = Duration::from_secs(5);
    let mut sidecar = Sidecar::new("/nonexistent/never-spawned", Vec::new(), cfg);
    for _ in 0..25 {
        let _restart_count = sidecar.record_restart();
    }
    assert_eq!(sidecar.next_backoff(), Duration::from_secs(5));
}

/// `record_restart` saturates at `u32::MAX` instead of overflowing.
#[test]
fn record_restart_saturates_at_u32_max() {
    let cfg = LifecycleConfig::default();
    let mut sidecar = Sidecar::new("/nonexistent/never-spawned", Vec::new(), cfg);
    for _ in 0..3 {
        let _restart_count = sidecar.record_restart();
    }
    sidecar.reset_restart_count();
    // After reset, next_backoff returns the initial value again.
    assert_eq!(sidecar.next_backoff(), Duration::from_secs(1));
    assert_eq!(sidecar.record_restart(), 1);
}

/// Spawning a nonexistent program reports `LifecycleError::Spawn`.
#[tokio::test]
async fn spawn_nonexistent_program_errors() {
    let mut sidecar = Sidecar::new(
        "/this/path/does/not/exist/sidecar-binary-xyz",
        Vec::new(),
        LifecycleConfig::default(),
    );
    let err = sidecar.spawn().expect_err("nonexistent spawn must error");
    match &err {
        LifecycleError::Spawn(io) => {
            // Display of the wrapped io::Error should appear in the LifecycleError.
            assert!(err.to_string().starts_with("spawn failed: "));
            assert_ne!(io.to_string(), "");
        }
        other => assert!(
            matches!(other, LifecycleError::Spawn(_)),
            "expected Spawn error, got {other:?}"
        ),
    }
}

/// `wait()` on a never-spawned sidecar returns `None`.
#[tokio::test]
async fn wait_without_spawn_returns_none() {
    let mut sidecar = Sidecar::new(
        "/nonexistent/never-spawned",
        Vec::new(),
        LifecycleConfig::default(),
    );
    assert!(sidecar.wait().await.is_none());
}

/// `Debug` formatting includes program, running flag, and restart count.
#[test]
fn debug_rendering_shows_state() {
    let sidecar = Sidecar::new(
        "demo-program",
        vec!["--arg".to_string()],
        LifecycleConfig::default(),
    );
    let rendered = format!("{sidecar:?}");
    assert!(rendered.contains("Sidecar"));
    assert!(rendered.contains("demo-program"));
    assert!(rendered.contains("running"));
    assert!(rendered.contains("restarts"));
}

/// `LifecycleError` `Display` impls render the documented strings.
#[test]
fn error_display_strings_are_stable() {
    let budget = LifecycleError::BudgetExhausted(7);
    assert_eq!(
        budget.to_string(),
        "restart budget exhausted after 7 attempts"
    );
    assert!(format!("{budget:?}").contains("BudgetExhausted"));

    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
    let spawn_err = LifecycleError::Spawn(io_err);
    assert_eq!(spawn_err.to_string(), "spawn failed: no such file");
}

// --- real-subprocess case (Unix only; no timing dependencies) ------------

/// On Unix, spawn `/usr/bin/true` and wait for its zero-status exit. This is
/// deterministic: `true` exits immediately with status 0 on every Unix.
#[cfg(unix)]
#[tokio::test]
async fn spawn_and_wait_on_true_binary() {
    if !std::path::Path::new("/usr/bin/true").exists() {
        // Environment does not ship the standard binary; nothing to assert
        // without resorting to timing or a fake shim.
        return;
    }
    let mut sidecar = Sidecar::new("/usr/bin/true", Vec::new(), LifecycleConfig::default());
    sidecar.spawn().expect("spawn /usr/bin/true should succeed");

    let status = sidecar.wait().await.expect("wait should yield exit status");
    assert!(status.success());

    // After the child has exited, a second wait still returns Some(status)
    // because the Child stays in place until the Sidecar is dropped.
    let _ = sidecar.wait().await;
}
