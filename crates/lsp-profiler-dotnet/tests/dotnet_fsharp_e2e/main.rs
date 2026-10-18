//! Full-stack .NET profiling of an F# program through the shared service. Tests
//! [PROF-DOTNET-CPU-SYMBOLS], [PROF-DOTNET-HEAP-GRAPH], [PROF-DOTNET-SOURCE],
//! [PROF-DOTNET-MEMORY], [PROF-MEMORY-CONFIDENCE] and [PROF-TESTING] on the IL
//! only F# emits.
//!
//! Real engine, real fixture, no mocks: `HotLeakAppFs` burns CPU in `Hot.spin`
//! and grows the module value `LeakCache.items` with records that hold union
//! cases and a generic record. Its line protocol adds the rest of the F# heap
//! zoo one shape at a time: orders (records over a single-case union, a union
//! case with fields, a list and a struct record), quotes behind a never-evicting
//! cache, never-removed event-handler closures, a two-node reference cycle and
//! retained arrays. Every member the compiler generates for a record or a union
//! carries only hidden sequence points, and a module value lives in a mangled
//! `items@<line>` field, so none of this is proven by the C# fixture.

#![allow(
    clippy::allow_attributes,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    missing_docs
)]

#[path = "../support/mod.rs"]
mod support;

mod cpu;
mod harness;
mod memory;
mod steps;
