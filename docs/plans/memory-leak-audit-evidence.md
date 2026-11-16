# Memory leak audit — executed evidence

[Audit and checklist](memory-leak-audit.md) · 2026-11-16

## [MEM-AUDIT-EVIDENCE] Reproduction

These are real test executions against unchanged production behavior. Every command exited 101 because the ownership assertion failed. The reference executable has only a `cfg(test)` module added; no retention fixes are applied.

### Sidecar allocation ownership

```sh
cargo test -p lspkit --test sidecar_memory_retention --locked -- --nocapture
```

```text
warning: unused workspace dependency `lsp-profiler-dotnet`
   --> Cargo.toml:162:1
    |
162 | lsp-profiler-dotnet = { version = "0.0.0-dev", path = "crates/lsp-profiler-dotnet" }
    | ^^^^^^^^^^^^^^^^^^^
    |
    = note: `cargo::unused_workspace_dependencies` is set to `warn` by default
help: consider removing the workspace dependency `lsp-profiler-dotnet`
warning: unused workspace dependency `lspkit-server`
   --> Cargo.toml:160:1
    |
160 | lspkit-server = { version = "0.0.0-dev", path = "crates/lspkit-server" }
    | ^^^^^^^^^^^^^
    |
help: consider removing the workspace dependency `lspkit-server`
warning: workspace (manifest) generated 2 warnings
   Compiling lspkit v0.0.0-dev (/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.11s
     Running tests/sidecar_memory_retention.rs (target/debug/build/lspkit/9697ec9472675c16/out/sidecar_memory_retention-9697ec9472675c16)

running 3 tests

thread 'abandoned_requests_release_their_channel_allocations' (64890414) panicked at crates/lspkit/tests/sidecar_memory_retention.rs:47:5:
assertion `left == right` failed: 32 abandoned oneshot senders still own at least 2097152 bytes of heap response slots; no receivers or request work remain
  left: 32
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread 'dropped_inbound_readers_release_their_decoder_allocations' (64890415) panicked at crates/lspkit/tests/sidecar_memory_retention.rs:121:5:
assertion `left == right` failed: 32 dropped inbound readers retain 65536-byte decoder allocations after the executor becomes idle
  left: 32
 right: 0
test dropped_inbound_readers_release_their_decoder_allocations ... FAILED
test abandoned_requests_release_their_channel_allocations ... FAILED
test resolved_requests_release_their_channel_allocations ... ok

failures:

failures:
    abandoned_requests_release_their_channel_allocations
    dropped_inbound_readers_release_their_decoder_allocations

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s

error: test failed, to rerun pass `-p lspkit --test sidecar_memory_retention`
```

### Dispatcher ingress ownership

```sh
cargo test -p lspkit-server --test dispatch_memory_retention --locked -- --nocapture
```

```text
warning: unused workspace dependency `lsp-profiler-dotnet`
   --> Cargo.toml:162:1
    |
162 | lsp-profiler-dotnet = { version = "0.0.0-dev", path = "crates/lsp-profiler-dotnet" }
    | ^^^^^^^^^^^^^^^^^^^
    |
    = note: `cargo::unused_workspace_dependencies` is set to `warn` by default
help: consider removing the workspace dependency `lsp-profiler-dotnet`
warning: unused workspace dependency `lspkit-server`
   --> Cargo.toml:160:1
    |
160 | lspkit-server = { version = "0.0.0-dev", path = "crates/lspkit-server" }
    | ^^^^^^^^^^^^^
    |
help: consider removing the workspace dependency `lspkit-server`
warning: workspace (manifest) generated 2 warnings
   Compiling lspkit-server v0.0.0-dev (/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit-server)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.96s
     Running tests/dispatch_memory_retention.rs (target/debug/build/lspkit-server/e783b3c44f3a9847/out/dispatch_memory_retention-e783b3c44f3a9847)

running 3 tests

thread 'unknown_method_releases_pre_registered_interest' (64917771) panicked at crates/lspkit-server/tests/dispatch_memory_retention.rs:44:5:
assertion `left == right` failed: terminal requests retained registry entries, each owning a 65536-byte input allocation
  left: "Dispatcher { handlers: 0, in_flight: 32 }"
 right: "Dispatcher { handlers: 0, in_flight: 0 }"
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test unknown_method_releases_pre_registered_interest ... FAILED
test dropped_futures_release_pre_registered_interest ... ok
test completed_requests_release_pre_registered_interest ... ok

failures:

failures:
    unknown_method_releases_pre_registered_interest

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

error: test failed, to rerun pass `-p lspkit-server --test dispatch_memory_retention`
```

### Actual reference server ownership

```sh
cargo test -p lspkit-server --example diagnostic-reference --features pull --locked memory_retention -- --nocapture
```

```text
warning: unused workspace dependency `lsp-profiler-dotnet`
   --> Cargo.toml:162:1
    |
162 | lsp-profiler-dotnet = { version = "0.0.0-dev", path = "crates/lsp-profiler-dotnet" }
    | ^^^^^^^^^^^^^^^^^^^
    |
    = note: `cargo::unused_workspace_dependencies` is set to `warn` by default
help: consider removing the workspace dependency `lsp-profiler-dotnet`
warning: unused workspace dependency `lspkit-server`
   --> Cargo.toml:160:1
    |
160 | lspkit-server = { version = "0.0.0-dev", path = "crates/lspkit-server" }
    | ^^^^^^^^^^^^^
    |
help: consider removing the workspace dependency `lspkit-server`
warning: workspace (manifest) generated 2 warnings
   Compiling smallvec v1.15.1
   Compiling tracing-core v0.1.36
   Compiling crossbeam-epoch v0.9.21
   Compiling crossbeam-deque v0.8.8
   Compiling rayon-core v1.13.0
   Compiling equivalent v1.0.2
   Compiling syn v3.0.9
   Compiling foldhash v0.2.0
   Compiling allocator-api2 v0.2.21
   Compiling typeid v1.0.3
   Compiling portable-atomic v1.15.0
   Compiling log v0.4.29
   Compiling tokio v1.54.1
   Compiling objc2-core-foundation v0.3.2
   Compiling regex-syntax v0.8.10
   Compiling parking_lot_core v0.9.12
   Compiling either v1.19.0
   Compiling bitflags v1.3.2
   Compiling same-file v1.0.6
   Compiling fluent-uri v0.1.4
   Compiling walkdir v2.5.0
   Compiling hashbrown v0.17.1
   Compiling tracing v0.1.44
   Compiling dashmap v6.2.1
   Compiling objc2-core-services v0.3.2
   Compiling parking_lot v0.12.5
   Compiling ropey v1.6.1
   Compiling crossbeam-queue v0.3.14
   Compiling notify-types v2.1.0
   Compiling thin-vec v0.2.21
   Compiling rustc-hash v2.1.3
   Compiling lazy_static v1.5.0
   Compiling boxcar v0.2.14
   Compiling inventory v0.3.25
   Compiling salsa-macro-rules v0.28.5
   Compiling intrusive-collections v0.10.3
   Compiling sharded-slab v0.1.7
   Compiling indexmap v2.14.0
   Compiling hashlink v0.12.2
   Compiling notify v8.2.1
   Compiling tracing-log v0.2.0
   Compiling thread_local v1.1.9
   Compiling nu-ansi-term v0.50.3
   Compiling regex-automata v0.4.14
   Compiling rayon v1.12.0
   Compiling matchers v0.2.0
   Compiling serde_derive v1.0.229
   Compiling thiserror-impl v2.0.21
   Compiling futures-macro v0.3.34
   Compiling salsa-macros v0.28.5
   Compiling async-trait v0.1.92
   Compiling futures-util v0.3.34
   Compiling thiserror v2.0.21
   Compiling tokio-util v0.7.20
   Compiling salsa v0.28.5
   Compiling serde v1.0.229
   Compiling rmp-serde v1.3.1
   Compiling tracing-serde v0.2.0
   Compiling lsp-types v0.97.0
   Compiling tracing-subscriber v0.3.23
   Compiling lspkit v0.0.0-dev (/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit)
   Compiling lspkit-server v0.0.0-dev (/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit-server)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 16.53s
     Running unittests examples/diagnostic-reference/main.rs (target/debug/build/lspkit-server/daef989524df31ce/out/diagnostic_reference-daef989524df31ce)

running 3 tests

thread 'session::memory_retention::registered_session_releases_its_source_allocations' (64843868) panicked at crates/lspkit-server/examples/diagnostic-reference/session/memory_retention.rs:46:5:
assertion `left == right` failed: handler closures retain the source buffer after every external Server owner is dropped
  left: 4
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test session::memory_retention::unregistered_session_releases_its_source_allocations ... ok
test session::memory_retention::registered_session_releases_its_source_allocations ... FAILED

thread 'session::memory_retention::closed_documents_release_their_source_buffers' (64843867) panicked at crates/lspkit-server/examples/diagnostic-reference/session/memory_retention.rs:60:5:
assertion `left == right` failed: didClose retained complete document text buffers after all 32 documents closed
  left: (32, 2097152)
 right: (0, 0)
test session::memory_retention::closed_documents_release_their_source_buffers ... FAILED

failures:

failures:
    session::memory_retention::closed_documents_release_their_source_buffers
    session::memory_retention::registered_session_releases_its_source_allocations

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p lspkit-server --example diagnostic-reference`
```
