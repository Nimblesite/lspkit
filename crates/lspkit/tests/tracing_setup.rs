//! Integration tests for the optional `tracing-setup` feature.

#![cfg(feature = "tracing-setup")]

use std::error::Error;

use lspkit::tracing_setup::{TracingBuilder, TracingError};

#[test]
fn builder_new_equals_default() {
    let from_new = format!("{:?}", TracingBuilder::new());
    let from_default = format!("{:?}", TracingBuilder::default());
    assert_eq!(from_new, from_default);
}

#[test]
fn install_with_invalid_filter_returns_filter_error() {
    let outcome = TracingBuilder::new()
        .with_filter("???not a directive???")
        .install();
    let filter_message = match &outcome {
        Err(TracingError::Filter(message)) => Some(message.clone()),
        _ => None,
    };
    assert!(
        filter_message.as_ref().is_some_and(|m| !m.is_empty()),
        "expected non-empty Filter error, got {outcome:?}"
    );
}

#[test]
fn install_with_unwritable_json_path_returns_open_file_error() {
    let outcome = TracingBuilder::new()
        .with_filter("info")
        .with_json_file("/this/path/does/not/exist/log.json")
        .install();
    let is_open_file = matches!(outcome, Err(TracingError::OpenFile(_)));
    assert!(is_open_file, "expected OpenFile error, got {outcome:?}");
    let display = outcome.err().map_or_else(String::new, |e| e.to_string());
    assert!(display.contains("could not open"), "got display: {display}");
}

#[test]
fn tracing_error_display_includes_filter_message() {
    let error = TracingError::Filter("bad directive".to_string());
    let display = error.to_string();
    assert!(display.contains("invalid filter"));
    assert!(display.contains("bad directive"));
}

#[test]
fn tracing_error_display_for_init_variant() {
    let error = TracingError::Init("already installed".to_string());
    let display = error.to_string();
    assert!(display.contains("could not install"));
    assert!(display.contains("already installed"));
}

#[test]
fn tracing_error_open_file_exposes_source() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
    let error = TracingError::OpenFile(io_err);
    assert!(error.source().is_some());
}

#[test]
fn install_then_install_returns_init_error() {
    let temp = std::env::temp_dir().join(format!("lspkit-tracing-{}.json", std::process::id()));
    let first = TracingBuilder::new()
        .with_filter("info")
        .with_json_file(&temp)
        .install();
    let second = TracingBuilder::new().with_filter("info").install();
    let _ = std::fs::remove_file(&temp);
    // Exactly one install can succeed in-process. The other must be an Init error.
    let init_seen =
        matches!(first, Err(TracingError::Init(_))) || matches!(second, Err(TracingError::Init(_)));
    assert!(
        init_seen,
        "expected one of the installs to fail with Init: first={first:?} second={second:?}"
    );
}

#[test]
fn install_without_explicit_filter_uses_rust_log_then_info_fallback() {
    // With no explicit directives and no RUST_LOG, the `info` fallback filter
    // must be selected; a filter error can never surface from this arm.
    let previous = std::env::var("RUST_LOG").ok();
    std::env::remove_var("RUST_LOG");
    let outcome = TracingBuilder::new().install();
    if let Some(value) = previous {
        std::env::set_var("RUST_LOG", value);
    }
    assert!(
        matches!(outcome, Ok(())) || matches!(outcome, Err(TracingError::Init(_))),
        "fallback filter must install or hit an already-installed subscriber: {outcome:?}"
    );
}
