//! End-to-end tests for the [`lspkit_server::capabilities`] builder.

use lspkit_server::{Capabilities, Feature, TextSync};
use lspkit_vfs::PositionEncoding;

#[test]
fn new_has_default_encoding_and_incremental_sync() {
    let caps = Capabilities::new();
    assert_eq!(caps.encoding(), PositionEncoding::default());
    assert_eq!(caps.text_sync_kind(), TextSync::Incremental);
    assert!(caps.features().is_empty());
}

#[test]
fn default_matches_new() {
    let lhs = Capabilities::default();
    let rhs = Capabilities::new();
    assert_eq!(lhs.encoding(), rhs.encoding());
    assert_eq!(lhs.text_sync_kind(), rhs.text_sync_kind());
    assert_eq!(lhs.features(), rhs.features());
}

#[test]
fn position_encoding_setter_overrides() {
    let caps = Capabilities::new().position_encoding(PositionEncoding::Utf8);
    assert_eq!(caps.encoding(), PositionEncoding::Utf8);
}

#[test]
fn text_sync_setter_overrides() {
    let caps = Capabilities::new().text_sync(TextSync::Full);
    assert_eq!(caps.text_sync_kind(), TextSync::Full);

    let caps_none = Capabilities::new().text_sync(TextSync::None);
    assert_eq!(caps_none.text_sync_kind(), TextSync::None);
}

#[test]
fn enable_records_feature_and_has_returns_true() {
    let caps = Capabilities::new()
        .enable(Feature::Completion)
        .enable(Feature::Hover);
    assert!(caps.has(Feature::Completion));
    assert!(caps.has(Feature::Hover));
    assert!(!caps.has(Feature::Definition));
    assert!(!caps.has(Feature::References));
    assert!(!caps.has(Feature::DiagnosticsPull));
}

#[test]
fn features_snapshot_is_sorted_and_deduped() {
    let caps = Capabilities::new()
        .enable(Feature::Hover)
        .enable(Feature::Completion)
        .enable(Feature::Hover);
    let features = caps.features();
    assert_eq!(features.len(), 2);
    assert!(features.contains(&Feature::Completion));
    assert!(features.contains(&Feature::Hover));
}

#[test]
fn builder_is_chainable_for_all_features() {
    let caps = Capabilities::new()
        .position_encoding(PositionEncoding::Utf16)
        .text_sync(TextSync::Incremental)
        .enable(Feature::Completion)
        .enable(Feature::Hover)
        .enable(Feature::Definition)
        .enable(Feature::References)
        .enable(Feature::DiagnosticsPull);
    assert_eq!(caps.features().len(), 5);
    assert!(caps.has(Feature::DiagnosticsPull));
}

#[test]
fn clone_preserves_state() {
    let caps = Capabilities::new()
        .enable(Feature::Hover)
        .position_encoding(PositionEncoding::Utf32);
    let cloned = caps.clone();
    assert_eq!(cloned.encoding(), PositionEncoding::Utf32);
    assert!(cloned.has(Feature::Hover));
}

#[test]
fn debug_format_does_not_panic() {
    let caps = Capabilities::new().enable(Feature::Completion);
    let rendered = format!("{caps:?}");
    assert!(rendered.contains("Capabilities"));
}

#[test]
fn text_sync_default_is_incremental() {
    assert_eq!(TextSync::default(), TextSync::Incremental);
}

#[test]
fn text_sync_copy_eq() {
    let kind = TextSync::Full;
    let copy = kind;
    assert_eq!(kind, copy);
}

#[test]
fn feature_ordering_is_stable() {
    let mut features = [Feature::Hover, Feature::Completion];
    features.sort();
    assert_eq!(features[0], Feature::Completion);
}
