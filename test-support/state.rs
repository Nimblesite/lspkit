//! Direct heap accounting shared by isolated, single-test executables. [MEM-FIX-SCOPE]

use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};
use std::alloc::System;

/// Each test executable has its own instrumented allocator and no sibling tests.
#[global_allocator]
static ALLOCATOR: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// Measure real allocation/deallocation calls before any registry inspection or teardown.
pub(crate) fn assert_releases_heap(operation: impl FnOnce()) {
    let region = Region::new(ALLOCATOR);
    operation();
    let stats = region.change();
    assert!(
        stats.bytes_allocated > 0,
        "the probe must allocate real heap memory"
    );
    assert_eq!(
        stats.bytes_allocated, stats.bytes_deallocated,
        "completed work still retains heap bytes: {stats:?}"
    );
}
