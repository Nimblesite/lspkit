//! Integration tests for the value types in `lspkit::engine`.

use std::path::PathBuf;

use lspkit::prelude::*;

#[test]
fn generation_zero_starts_at_zero() {
    assert_eq!(Generation::ZERO.get(), 0);
}

#[test]
fn generation_new_wraps_value() {
    let generation = Generation::new(42);
    assert_eq!(generation.get(), 42);
}

#[test]
fn generation_next_increments() {
    let generation = Generation::new(7).next();
    assert_eq!(generation.get(), 8);
}

#[test]
fn generation_next_saturates_at_max() {
    let generation = Generation::new(u64::MAX).next();
    assert_eq!(generation.get(), u64::MAX);
}

#[test]
fn generation_ordering_is_monotonic() {
    let zero = Generation::ZERO;
    let one = zero.next();
    let two = one.next();
    assert!(zero < one);
    assert!(one < two);
    assert_eq!(zero, Generation::new(0));
}

#[test]
fn generation_display_uses_gen_prefix() {
    let formatted = format!("{}", Generation::new(3));
    assert_eq!(formatted, "gen#3");
}

#[test]
fn generation_debug_includes_value() {
    let formatted = format!("{:?}", Generation::new(9));
    assert!(formatted.contains('9'));
}

#[test]
fn generation_copy_and_hash_work() {
    use std::collections::HashSet;
    let gen_a = Generation::new(5);
    let gen_b = gen_a;
    let mut set = HashSet::new();
    set.insert(gen_a);
    assert!(set.contains(&gen_b));
}

#[test]
fn snapshot_new_stores_data_and_generation() {
    let snap = Snapshot::new(Generation::new(2), vec![1, 2, 3]);
    assert_eq!(snap.generation, Generation::new(2));
    assert_eq!(snap.data, vec![1, 2, 3]);
}

#[test]
fn snapshot_clone_preserves_contents() {
    let snap = Snapshot::new(Generation::new(11), "report".to_string());
    let cloned = snap.clone();
    assert_eq!(cloned.generation, snap.generation);
    assert_eq!(cloned.data, snap.data);
}

#[test]
fn snapshot_debug_contains_generation_label() {
    let snap = Snapshot::new(Generation::new(4), 0u32);
    let formatted = format!("{snap:?}");
    assert!(formatted.contains("Snapshot"));
}

#[test]
fn cause_variants_all_constructible() {
    let causes = [Cause::Initial, Cause::Rescan, Cause::External];
    for cause in &causes {
        let formatted = format!("{cause:?}");
        assert!(!formatted.is_empty());
    }
}

#[test]
fn cause_clone_round_trip() {
    let cause = Cause::Rescan;
    let cloned = cause.clone();
    let formatted = format!("{cloned:?}");
    assert!(formatted.contains("Rescan"));
}

#[test]
fn generation_event_new_stores_fields() {
    let event = GenerationEvent::new(Generation::new(6), Cause::External);
    assert_eq!(event.generation, Generation::new(6));
    assert!(matches!(event.cause, Cause::External));
}

#[test]
fn generation_event_clone_is_equivalent() {
    let event = GenerationEvent::new(Generation::new(1), Cause::Initial);
    let cloned = event.clone();
    assert_eq!(cloned.generation, event.generation);
}

#[test]
fn rescan_scope_all_debug_renders() {
    let scope = RescanScope::All;
    let formatted = format!("{scope:?}");
    assert!(formatted.contains("All"));
}

#[test]
fn rescan_scope_paths_stores_listed_paths() {
    let paths = vec![PathBuf::from("/a"), PathBuf::from("/b/c")];
    let scope = RescanScope::Paths(paths.clone());
    let extracted = match &scope {
        RescanScope::Paths(stored) => Some(stored.clone()),
        _ => None,
    };
    assert_eq!(extracted, Some(paths));
}

#[test]
fn rescan_ticket_new_returns_generation() {
    let ticket = RescanTicket::new(Generation::new(13));
    assert_eq!(ticket.generation(), Generation::new(13));
}

#[test]
fn rescan_ticket_clone_preserves_generation() {
    let ticket = RescanTicket::new(Generation::new(17));
    let cloned = ticket.clone();
    assert_eq!(cloned.generation(), ticket.generation());
}

#[test]
fn progress_noop_report_is_silent() {
    let progress = Progress::noop();
    progress.report("starting", None);
    progress.report("midway", Some(50));
    progress.report("complete", Some(100));
}

#[test]
fn progress_default_is_noop() {
    let progress = Progress::default();
    progress.report("x", Some(0));
}

#[test]
fn progress_clone_works() {
    let progress = Progress::noop();
    let cloned = progress.clone();
    cloned.report("cloned", None);
}

#[test]
fn progress_debug_renders_struct_name() {
    let progress = Progress::noop();
    let formatted = format!("{progress:?}");
    assert!(formatted.contains("Progress"));
}
