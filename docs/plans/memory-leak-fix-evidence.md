# Memory leak repair evidence — 2026-11-16

[MEM-FIX-SCOPE](memory-leak-fixes.md#mem-fix-scope-scope). Executed test output before and after repairs; compiler progress omitted. Each red result is an allocation-retention assertion, not a build failure.

## sidecar-red

Command: `cargo test -p lspkit --test sidecar_memory_retention --locked -- --nocapture`

```text
running 3 tests

thread 'abandoned_requests_release_their_channel_allocations' (65454926) panicked at crates/lspkit/tests/sidecar_memory_retention.rs:47:5:
assertion `left == right` failed: 32 abandoned oneshot senders still own at least 2097152 bytes of heap response slots; no receivers or request work remain
  left: 32
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test abandoned_requests_release_their_channel_allocations ... FAILED

thread 'dropped_inbound_readers_release_their_decoder_allocations' (65454927) panicked at crates/lspkit/tests/sidecar_memory_retention.rs:118:5:
assertion `left == right` failed: 32 dropped inbound readers retain 65536-byte decoder allocations after the executor becomes idle
  left: 32
 right: 0
test dropped_inbound_readers_release_their_decoder_allocations ... FAILED
test resolved_requests_release_their_channel_allocations ... ok

failures:

failures:
    abandoned_requests_release_their_channel_allocations
    dropped_inbound_readers_release_their_decoder_allocations

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.71s

error: test failed, to rerun pass `-p lspkit --test sidecar_memory_retention`
```

## dispatch-red

Command: `cargo test -p lspkit-server --test dispatch_memory_retention --locked -- --nocapture`

```text
running 3 tests

thread 'unknown_method_releases_pre_registered_interest' (65476304) panicked at crates/lspkit-server/tests/dispatch_memory_retention.rs:44:5:
assertion `left == right` failed: terminal requests retained registry entries, each owning a 65536-byte input allocation
  left: "Dispatcher { handlers: 0, in_flight: 32 }"
 right: "Dispatcher { handlers: 0, in_flight: 0 }"
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test unknown_method_releases_pre_registered_interest ... FAILED
test completed_requests_release_pre_registered_interest ... ok
test dropped_futures_release_pre_registered_interest ... ok

failures:

failures:
    unknown_method_releases_pre_registered_interest

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p lspkit-server --test dispatch_memory_retention`
```

## reference-red

Command: `cargo test -p lspkit-server --example diagnostic-reference --features pull --locked memory_retention -- --nocapture`

```text
running 3 tests

thread 'session::memory_retention::registered_session_releases_its_source_allocations' (65513362) panicked at crates/lspkit-server/examples/diagnostic-reference/session/memory_retention.rs:49:5:
assertion `left == right` failed: handler closures retain the source buffer after every external Server owner is dropped
  left: 4
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test session::memory_retention::unregistered_session_releases_its_source_allocations ... ok
test session::memory_retention::registered_session_releases_its_source_allocations ... FAILED

thread 'session::memory_retention::closed_documents_release_their_source_buffers' (65513361) panicked at crates/lspkit-server/examples/diagnostic-reference/session/memory_retention.rs:70:5:
assertion `left == right` failed: didClose retained complete document text buffers after all 32 documents closed
  left: (32, 2097152)
 right: (0, 0)
test session::memory_retention::closed_documents_release_their_source_buffers ... FAILED

failures:

failures:
    session::memory_retention::closed_documents_release_their_source_buffers
    session::memory_retention::registered_session_releases_its_source_allocations

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

error: test failed, to rerun pass `-p lspkit-server --example diagnostic-reference`
```

## sidecar-heap-red

Command: `cargo test -p lspkit --test sidecar_heap_leak -- --nocapture`

```text
running 1 test

thread 'abandoned_receivers_free_heap_without_a_later_correlator_call' (65514815) panicked at crates/lspkit/tests/../../../test-support/state.rs:16:5:
assertion `left == right` failed: completed work still retains heap bytes: Stats { allocations: 32, deallocations: 0, reallocations: 0, bytes_allocated: 2099200, bytes_deallocated: 0, bytes_reallocated: 0 }
  left: 2099200
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test abandoned_receivers_free_heap_without_a_later_correlator_call ... FAILED

failures:

failures:
    abandoned_receivers_free_heap_without_a_later_correlator_call

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s

error: test failed, to rerun pass `-p lspkit --test sidecar_heap_leak`
```

## dispatch-heap-red

Command: `cargo test -p lspkit-server --test dispatch_heap_leak --locked -- --nocapture`

```text
running 1 test

thread 'terminal_lookup_errors_free_heap_before_shutdown_or_registry_inspection' (65526701) panicked at crates/lspkit-server/tests/../../../test-support/state.rs:16:5:
assertion `left == right` failed: completed work still retains heap bytes: Stats { allocations: 192, deallocations: 32, reallocations: 0, bytes_allocated: 2105792, bytes_deallocated: 224, bytes_reallocated: 0 }
  left: 2105792
 right: 224
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test terminal_lookup_errors_free_heap_before_shutdown_or_registry_inspection ... FAILED

failures:

failures:
    terminal_lookup_errors_free_heap_before_shutdown_or_registry_inspection

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p lspkit-server --test dispatch_heap_leak`
```

## watcher-red

Command: `cargo test -p lspkit-server --example diagnostic-reference --features pull --locked initialized_watcher_releases_session_source_allocations -- --nocapture`

```text
running 1 test

thread 'session::watch_memory_retention::initialized_watcher_releases_session_source_allocations' (65759049) panicked at crates/lspkit-server/examples/diagnostic-reference/session/watch_memory_retention.rs:36:5:
assertion `left == right` failed: initialized watcher retains the real source buffer after session teardown
  left: 1
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test session::watch_memory_retention::initialized_watcher_releases_session_source_allocations ... FAILED

failures:

failures:
    session::watch_memory_retention::initialized_watcher_releases_session_source_allocations

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s

error: test failed, to rerun pass `-p lspkit-server --example diagnostic-reference`
```

## sidecar-green

Command: `cargo test -p lspkit --test sidecar_memory_retention --test sidecar_heap_leak --test sidecar_correlator --locked -- --nocapture`

```text
running 9 tests
test pending_count_reflects_registrations ... ok
test debug_rendering_shows_pending_count ... ok
test default_constructor_is_usable ... ok
test request_id_traits_are_derived ... ok
test error_display_strings_are_stable ... ok
test allocated_ids_are_monotonic_from_one ... ok
test out_of_order_responses_match_correct_receivers ... ok
test resolving_unknown_id_errors ... ok
test resolve_against_dropped_receiver_errors ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/sidecar_heap_leak.rs (target/debug/build/lspkit/3462b822b4c0ff8d/out/sidecar_heap_leak-3462b822b4c0ff8d)

running 1 test
test abandoned_receivers_free_heap_without_a_later_correlator_call ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s

     Running tests/sidecar_memory_retention.rs (target/debug/build/lspkit/b0ac403ea636af54/out/sidecar_memory_retention-b0ac403ea636af54)

running 3 tests
test abandoned_requests_release_their_channel_allocations ... ok
test dropped_inbound_readers_release_their_decoder_allocations ... ok
test resolved_requests_release_their_channel_allocations ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s

```

## dispatch-green

Command: `cargo test -p lspkit-server --test dispatch_memory_retention --test dispatch_heap_leak --test dispatch --locked -- --nocapture`

```text
running 12 tests
test cancel_trips_token_for_inflight_request ... ok
test default_dispatcher_matches_new ... ok
test cancel_unknown_id_is_noop ... ok
test debug_shows_handler_count ... ok
test unknown_method_returns_error ... ok
test registered_handler_receives_params_and_returns_ok ... ok
test handler_result_debug_works ... ok
test dispatcher_clone_shares_handlers ... ok
test registered_handler_can_return_jsonrpc_error ... ok
test dispatch_removes_cancellation_on_completion ... ok
test re_registering_replaces_handler ... ok
test dispatch_error_display_includes_method ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/dispatch_heap_leak.rs (target/debug/build/lspkit-server/aa8702b73f2d6345/out/dispatch_heap_leak-aa8702b73f2d6345)

running 1 test
test terminal_lookup_errors_free_heap_before_shutdown_or_registry_inspection ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/dispatch_memory_retention.rs (target/debug/build/lspkit-server/4a4addbb18ef7a80/out/dispatch_memory_retention-4a4addbb18ef7a80)

running 3 tests
test dropped_futures_release_pre_registered_interest ... ok
test unknown_method_releases_pre_registered_interest ... ok
test completed_requests_release_pre_registered_interest ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

```

## watcher-green

Command: `cargo test -p lspkit-server --example diagnostic-reference --features pull --locked memory_retention -- --nocapture`

```text
running 4 tests
test session::memory_retention::unregistered_session_releases_its_source_allocations ... ok
test session::memory_retention::registered_session_releases_its_source_allocations ... ok
test session::memory_retention::closed_documents_release_their_source_buffers ... ok
test session::watch_memory_retention::initialized_watcher_releases_session_source_allocations ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s

```

## Original proof integrity

- `crates/lspkit/tests/sidecar_memory_retention.rs`: `fd4a381a50be75e542fbd6131b9fa06ee0b1b259f3b2cc157fe9c427e84b0237`
- `crates/lspkit-server/tests/dispatch_memory_retention.rs`: `79d9fbb9cfe4a36bbf7a32309662f077296c7acdc004c94d752c3810665f8bde`
- `crates/lspkit-server/examples/diagnostic-reference/session/memory_retention.rs`: `b24215fcadfbc2c2ad64d78f79647cb8751a6cfa061d1e5fafc7758d89f41364`

## Full validation

`make ci` exited 0 on 2026-11-16, including analysis, tests, coverage, and release build. `make fmt CHECK=1` exited 0. Original proof hashes above were rechecked after the full run and match.

```text
Summary [  38.669s] 496 tests run: 496 passed, 0 skipped
OK:   lsp-profiler         95.54% >= 94%
OK:   lsp-profiler-dotnet  92.76% >= 92%
OK:   lspkit               99.17% >= 99%
OK:   lspkit-server        97.12% >= 97%
OK:   lspkit-server::live  94.66% >= 94%
OK:   lspkit-server::mcp   100.00% >= 100%
OK:   lspkit::config       100.00% >= 100%
OK:   lspkit::sidecar      99.25% >= 99%
OK:   lspkit::treesitter   95.00% >= 95%
OK:   lspkit::vfs          100.00% >= 100%
Statements   : 99.44% ( 1424/1432 )
Branches     : 96.48% ( 247/256 )
Functions    : 99.12% ( 113/114 )
Lines        : 99.44% ( 1424/1432 )
```
