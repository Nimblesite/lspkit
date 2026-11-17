# Red→green evidence for issues #21, #26, #48 and #49

Every fix below is proven the same way: put the pre-fix code back, run the named test and watch it fail on the issue's exact symptom, then restore the code and watch the same test pass. Run all commands from the repo root on branch `fix/issues-21-26-48-49`. Results were captured on macOS (aarch64), Rust 1.100.0.

The reference-server suites drive the real compiled `diagnostic-reference` binary. Build it after every source change, because it links `lspkit`:

```sh
cargo build -p lspkit-server --example diagnostic-reference --features pull --locked
export LSPKIT_REFERENCE_BIN=$PWD/target/debug/examples/diagnostic-reference
```

| Issue | Fixed in | Released | Regression test |
|---|---|---|---|
| #21 truncated frame read as clean EOF | `5590a52` (PR #27, added `next_frame` in `crates/lspkit-sidecar/src/inbound.rs`, now `crates/lspkit/src/sidecar/inbound.rs:109`) | v0.1.0 onward, including 0.5.0 | `crates/lspkit/tests/sidecar_disconnect_session.rs:79` `native_disconnects_preserve_prefix_and_payload_failures_after_valid_traffic` |
| #26 cancellation test drops the earlier partial | `741e806` (PR #33, `crates/lspkit-server/tests/cli_sessions/dispatcher.rs:90`) | v0.1.0 onward, including 0.5.0 | `crates/lspkit-server/tests/cli_sessions/dispatcher.rs:119` `cancel_during_a_scan_stops_the_stream_with_the_standard_error` |
| #49 URI host read as a path folder; UNC and `\\?\` paths | `384b341` (PR #50) | not yet; ships in v0.6.0 | `crates/lspkit-server/tests/uri_authority.rs` `localhost_authority_is_the_host_not_a_folder`; the Windows plan's table in `crates/lspkit-server/src/uri_cases.rs`, run by `src/uri_style_tests.rs` (every OS) and `tests/uri_windows.rs` (Windows, public API); see [Windows](#windows) |
| #48 no server→client requests | this branch | ships in v0.6.0 | `crates/lspkit-server/tests/outbound.rs:84` and `:183`, `crates/lspkit-server/tests/outbound_heap_leak.rs:66`, `crates/lspkit/tests/sidecar_correlator.rs:211` |

## #21: a truncated frame is an error, not a clean end of stream

The old reader turned every `UnexpectedEof` into "stream ended", including a peer that closes mid-prefix or mid-payload. The fix reads one byte first: zero bytes is a clean end, and anything else goes through the frame decoder, so a torn frame surfaces as an error.

Red: put the old behaviour back, rebuild the reference binary, run the test.

```diff
diff --git a/crates/lspkit/src/sidecar/inbound.rs b/crates/lspkit/src/sidecar/inbound.rs
index 0058be5..eb47a94 100644
--- a/crates/lspkit/src/sidecar/inbound.rs
+++ b/crates/lspkit/src/sidecar/inbound.rs
@@ -3,7 +3,7 @@
 //! Codec and envelope classification belong to the caller. This layer only
 //! separates response flow from unsolicited backend events.
 
-use tokio::io::{AsyncRead, AsyncReadExt};
+use tokio::io::{AsyncRead};
 use tokio::sync::mpsc;
 use tokio::task::JoinHandle;
 
@@ -110,18 +110,13 @@ async fn next_frame<R: AsyncRead + Unpin>(
     reader: &mut R,
     max_frame: usize,
 ) -> Result<Option<Vec<u8>>, String> {
-    let mut first = [0];
-    if reader
-        .read(&mut first)
-        .await
-        .map_err(|error| error.to_string())?
-        == 0
-    {
-        return Ok(None);
+    match read_frame(reader, max_frame).await {
+        Ok(frame) => Ok(Some(frame)),
+        Err(crate::sidecar::transport::TransportError::Io(e))
+            if e.kind() == std::io::ErrorKind::UnexpectedEof =>
+        {
+            Ok(None)
+        }
+        Err(error) => Err(error.to_string()),
     }
-    let mut prefixed = std::io::Cursor::new(first).chain(reader);
-    read_frame(&mut prefixed, max_frame)
-        .await
-        .map(Some)
-        .map_err(|error| error.to_string())
 }
```

```sh
git apply red-21.patch   # the diff above
cargo build -p lspkit-server --example diagnostic-reference --features pull --locked
cargo nextest run -p lspkit --all-features --test sidecar_disconnect_session
```

```text
        FAIL [   0.549s] (2/2) lspkit::sidecar_disconnect_session native_disconnects_preserve_prefix_and_payload_failures_after_valid_traffic
    thread 'native_disconnects_preserve_prefix_and_payload_failures_after_valid_traffic' (1754426) panicked at crates/lspkit/tests/sidecar_disconnect_session.rs:48:5:
      left: 0
     right: 1
        FAIL [   0.549s] (2/2) lspkit::sidecar_disconnect_session native_disconnects_preserve_prefix_and_payload_failures_after_valid_traffic
```

`sidecar_disconnect_session.rs:48` asserts exactly one terminal read error: the old code produced zero, so the torn frame looked like a clean EOF. Green, after `git checkout crates/lspkit/src/sidecar/inbound.rs` and a rebuild:

```text
     Summary [   0.640s] 2 tests run: 2 passed, 0 skipped
```

## #26: the earlier partial result is kept when the scan is cancelled

#26 was a bug in the test, not the server: the test consumed the first partial as a barrier and then checked only the partials that came after it. Correct server behaviour therefore failed the count. The fix chains the barrier back into the checked list. The server streams one partial per document before the hold, and the test asserts all 64, that every one is a distinct scan document, that the held document never streamed, and that the terminal reply is the standard `-32800`.

Red: put the old assertion back and run it against the correct server.

```diff
diff --git a/crates/lspkit-server/tests/cli_sessions/dispatcher.rs b/crates/lspkit-server/tests/cli_sessions/dispatcher.rs
index d521142..d75f212 100644
--- a/crates/lspkit-server/tests/cli_sessions/dispatcher.rs
+++ b/crates/lspkit-server/tests/cli_sessions/dispatcher.rs
@@ -87,7 +87,8 @@ fn assert_streamed_up_to_the_hold(
     barrier: &Value,
     partials: &[Value],
 ) {
-    let streamed: Vec<&Value> = std::iter::once(barrier).chain(partials).collect();
+    let _ = barrier;
+    let streamed: Vec<&Value> = partials.iter().collect();
     for partial in &streamed {
         assert_partial_document(partial);
     }
```

```sh
cargo nextest run -p lspkit-server --all-features --test diagnostic_reference cancel_during_a_scan
```

```text
        FAIL [   0.027s] (1/1) lspkit-server::diagnostic_reference dispatcher::cancel_during_a_scan_stops_the_stream_with_the_standard_error
    thread 'dispatcher::cancel_during_a_scan_stops_the_stream_with_the_standard_error' (1757217) panicked at crates/lspkit-server/tests/cli_sessions/dispatcher.rs:100:5:
      left: 63
     right: 64
        FAIL [   0.027s] (1/1) lspkit-server::diagnostic_reference dispatcher::cancel_during_a_scan_stops_the_stream_with_the_standard_error
```

63 is not 64 because the old check dropped `scan-0000.json`, the earlier partial. Green, after `git checkout crates/lspkit-server/tests/cli_sessions/dispatcher.rs`:

```text
     Summary [   0.028s] 1 test run: 1 passed, 25 skipped
```

## #49: a URI's host is a host, never a path folder

Red: restore the pre-fix `uri.rs` and run the public-API test.

```sh
git show 384b341^:crates/lspkit-server/src/uri.rs > crates/lspkit-server/src/uri.rs
cargo nextest run -p lspkit-server --test uri_authority
```

```text
        FAIL [   0.007s] (1/1) lspkit-server::uri_authority localhost_authority_is_the_host_not_a_folder
    thread 'localhost_authority_is_the_host_not_a_folder' (1760196) panicked at crates/lspkit-server/tests/uri_authority.rs:18:9:
      left: "/localhost/tmp/a.rs"
     right: "/tmp/a.rs"
```

This is the issue's row `file://localhost/tmp/a.rs` → `/localhost/tmp/a.rs`. Green, after `git checkout crates/lspkit-server/src/uri.rs`:

```text
     Summary [   0.015s] 1 test run: 1 passed, 0 skipped
```

The Windows rows (UNC `file://server/share/a.rs` ↔ `\\server\share\a.rs`, `file://wsl.localhost/…`, and `\\?\C:\…` and `\\?\UNC\…` → URI) and a percent-encoded host (`file://build%2Dhost/share%20one/a.rs` ↔ `\\build-host\share one\a.rs`) are unit tests in `uri_style_tests.rs`. They call the Windows conversion directly, so Linux and macOS CI run them on every PR. CI has no Windows runner, so the `cfg(windows)` selection of that branch is not executed in CI.

## #48: the server can send its own requests to the editor

`lspkit_server::Outbound` allocates a numeric JSON-RPC id, returns the request frame and an `OutboundResponse` future, and `resolve` delivers the editor's answer. The tests run over real Content-Length framing on a duplex stream.

- A `window/showDocument` request reaches the editor with its method, params and id, the editor's `{"success": true}` comes back to the caller, and ids are never reused.
- A `null` result and a frame with no `result` key both resolve to `Value::Null` (`workspace/diagnostic/refresh`).
- An editor `JsonRpcError` reaches the caller with its code, message and data. It renders as `JSON-RPC error -32603: document refused` through `Display` and through `Box<dyn std::error::Error>`.
- Dropping a wait unregisters it at once (`pending_count` 1 → 0), and the late answer is `UnknownId`.
- String, negative and never-allocated ids are `UnknownId`. Notifications, requests and `"id": null` errors are `NotAResponse`.
- `outbound_heap_leak.rs` runs 32 rounds of answered, refused, abandoned and foreign lifetimes and asserts that allocated heap bytes equal freed bytes. Replacing `drop(abandoned)` with `std::mem::forget(abandoned)` makes it fail, which shows the test detects retention.
- A parameterless request (`workspace/diagnostic/refresh`) omits `params` on the wire instead of sending `"params": null`, which JSON-RPC 2.0 forbids. `Message::request` and `Message::notification` both omit `null` params. Red before the fix: `outbound.rs` asserted `left: Some(Null) right: None` on `{"id":3,"jsonrpc":"2.0","method":"workspace/diagnostic/refresh","params":null}`.
- A duplicate answer for an already answered id is `UnknownId`, and the first answer still reaches its caller.
- Dropping the `Outbound` handle (server shutdown) ends every live wait with `RecvError` instead of hanging it.
- A client that never answers keeps exactly one entry until the caller drops its wait. Dropping is how a server cancels its own request; sending `$/cancelRequest` to the editor stays the caller's choice.
- `sidecar::RequestId::from(raw)` rebuilds an id from the raw wire number and resolves.

### Behavioural red: an answer wakes only its own request

`outbound.rs` `out_of_order_answers_wake_only_the_wait_with_their_id` sends three requests, and the editor answers them third, first, second. Each wait must receive the answer carrying its own id. Mutate `resolve` so it rotates ids, so that every answer wakes another pending request while every resolve still succeeds:

```diff
diff --git a/crates/lspkit-server/src/outbound.rs b/crates/lspkit-server/src/outbound.rs
index 73cb512..9eeedd2 100644
--- a/crates/lspkit-server/src/outbound.rs
+++ b/crates/lspkit-server/src/outbound.rs
@@ -71,7 +71,7 @@ impl Outbound {
         let Some(raw) = correlated(&id) else {
             return Err(OutboundError::UnknownId(id));
         };
-        self.correlator.resolve(raw.into(), answer).map_err(
+        self.correlator.resolve(((raw % 3) + 1).into(), answer).map_err(
             |_answered_or_abandoned: CorrelationError<OutboundResult>| OutboundError::UnknownId(id),
         )
     }
```

```text
    thread 'out_of_order_answers_wake_only_the_wait_with_their_id' (1855494) panicked at crates/lspkit-server/tests/outbound.rs:333:9:
    assertion `left == right` failed: wait for Number(1) got another request's answer
      left: Object {"answered": String("third")}
     right: Object {"answered": String("first")}
     Summary [   0.010s] 1 test run: 0 passed, 1 failed, 2 skipped
```

Restored, all outbound tests pass.

### Behavioural red: ids past `i64::MAX` fail instead of colliding

The unit test `outbound::tests::ids_past_the_largest_json_rpc_number_fail_instead_of_colliding` (`crates/lspkit-server/src/outbound.rs`) checks the id boundary directly, because reaching 2^63 real registrations is not feasible. Before the fix `wire_id` saturated:

```text
panicked at crates/lspkit-server/src/outbound.rs:132:41:
no wire number left: Number(9223372036854775807)
```

So id 2^63 went out as `9223372036854775807`, the same number as id 2^63 − 1. Now `request` returns `OutboundError::IdsExhausted(raw)` and drops the registration.

### Compile red: the API did not exist

Remove the fix and keep the tests.

```sh
git diff main -- crates/lspkit-server/src crates/lspkit/src > fix-48.patch
git apply -R fix-48.patch
cargo test -p lspkit-server --test outbound --test outbound_heap_leak --no-run
cargo test -p lspkit --test sidecar_correlator --no-run
```

```text
error[E0432]: unresolved imports `lspkit_server::Outbound`, `lspkit_server::OutboundError`
error[E0432]: unresolved imports `lspkit_server::Outbound`, `lspkit_server::OutboundError`, `lspkit_server::OutboundResponse`
    |                                     --------------- ^^^ expected `RequestId`, found `u64`
    |                  --------------- ^^^ expected `RequestId`, found `u64`
    |                  --------------- ^^^^^^^^^^^ expected `RequestId`, found `u64`
    |                --------------- ^^^ expected `RequestId`, found `u64`
error[E0308]: mismatched types
```

Green, after `git apply fix-48.patch`:

```text
     Summary [   0.010s] 3 tests run: 3 passed, 0 skipped
     Summary [   0.012s] 12 tests run: 12 passed, 0 skipped
```

## #53: an oversized `Content-Length` is an error, never a panic or an up-front allocation

[JSONRPC-FRAME-LIMIT]. Red against a stub (`read_message_within` existing but ignoring its cap), so the tests compile and fail on behaviour:

```text
oversized_content_length_is_rejected_before_any_body_is_read ... FAILED
  panicked at library/alloc/src/raw_vec/mod.rs:29:5: capacity overflow
a_cap_admits_its_limit_and_rejects_one_byte_more ... FAILED
  one byte over the cap: Message { method: Some("x"), .. }
a_short_body_allocates_what_arrived_not_what_was_claimed ... FAILED   (tests/jsonrpc_frame_heap.rs)
  a 25-byte body claiming 67108864 allocated 67117299 bytes
```

Green after the cap check and the incremental `take(..).read_to_end` body read: `tests/jsonrpc.rs` 20/20, `tests/jsonrpc_frame_heap.rs` 1/1 (under 64 KiB allocated, and all of it released). Clippy is clean on the host and on `x86_64-pc-windows-msvc`.

## #52: closing one document keeps every other memo warm

[LSP-PULL-FORGET]. Red against a stub `forget` that returns `Ok(())` and does nothing:

```text
forgetting_one_document_keeps_every_other_memo_warm ... FAILED
  a forgotten document recomputes  left: 1  right: 2
forget_is_idempotent_and_a_recycled_input_never_replays_old_truth ... FAILED
  B recomputes after its forget  left: 1  right: 2
open_forget_cycles_of_distinct_uris_release_their_heap ... FAILED   (tests/pull_forget_heap.rs)
  completed work still retains heap bytes: allocations 12089, deallocations 4072,
  bytes_allocated 1439392, bytes_deallocated 334016
```

Green with retired-slot reuse: `pull_forget` 2/2 (B and C remain cache hits with unchanged result ids, A recomputes once, a recycled slot reports only its new URI), `pull_forget_heap` 1/1 (1000 distinct open/forget cycles, 17000 allocations and 17000 deallocations, equal bytes), and `pull_store` 4/4. Disabling `forget` again turns all three red.

### #53 follow-up: the header block is capped too

DeslopClaude1 measured 544 MB RSS for a 512 MiB header line with no newline at `0dc2f04`. The red came from stub `HeaderTooLarge` and `MAX_HEADER_BYTES` items that nothing enforced:

```text
header_blocks_past_the_cap_are_rejected_and_one_at_the_cap_decodes ... FAILED
  one endless line: Closed
a_short_body_allocates_what_arrived_not_what_was_claimed ... FAILED   (tests/jsonrpc_frame_heap.rs)
  a 524288-byte header line under a 8192-byte cap allocated 1056768 bytes
```

Green once every header line is read through `take(budget)`: `tests/jsonrpc.rs` 21/21 and `jsonrpc_frame_heap` 1/1, each under 64 KiB. The test covers one endless line, endless `X: y` lines, a block exactly at the cap reaching the body, and one byte over it.

### #52 follow-up: closes made during a pull, and repeated closes

SharpLsp1 found that deleting the release `analyze` call in `d2a22c4` left all tests green. `029e583` (Windows agent) then retired documents on close via `try_lock`, using an `open` flag and a retry write. On top of it, this change retires closes queued during an in-flight pull as soon as that pull releases the state, instead of waiting for a later pull, and makes the queue a set, so repeated closes are one entry. `pull_forget_heap` asserts that closing 32 idle 64 KiB documents frees every report byte, that a close during an in-flight pull is freed by that pull as it finishes, and that 1000 repeated closes keep no heap. Red at `d2a22c4`:

```text
closing 32 idle documents freed 0 of their 2097152 report bytes
```

On the merged code, each fix line mutated on its own turns the test red:

```text
release `analyze` removed        → freed 7072 of their 2097152 report bytes
retry write removed              → freed 2050592 of their 2097152 report bytes
retire-on-close removed          → freed 1532 of their 2097152 report bytes
post-query retire removed        → a close during a pull freed 160 of its 65536 report bytes
```

`set_open(..).to(false)` removed → `forgetting_one_document_keeps_every_other_memo_warm` fails with "a close never runs the compiler: left 4, right 3" (SharpLsp1's mutant, now killed).

## Windows

Captured natively on Microsoft Windows 11 Enterprise (NT 10.0.26300, AMD64), Rust 1.100.0 `x86_64-pc-windows-msvc`, .NET SDK 10. On this build, `cfg(windows)` selects the real Windows code paths. Linux rows were checked in WSL (`UbuntuDev`) and by CI.

### #49: the issue's Windows verification plan

The plan's tables live once, in `crates/lspkit-server/src/uri_cases.rs`: 35 round trips, 14 paths → URIs, 9 rejected paths and 23 rejected URIs. Two suites run them:

- `src/uri_style_tests.rs` runs them through `Style::Windows`, on every OS.
- `tests/uri_windows.rs` (`#![cfg(windows)]`) runs them through the public `uri_to_path` and `path_to_uri` only, so it also compiles against the pre-fix crate. It adds real-filesystem rows:
  - `std::fs::canonicalize` output for a temp file, a temp dir, a dir with spaces, Unicode and reserved-character names;
  - a file under a path longer than 300 characters, created through `\\?\`;
  - files named `trailing dot.`, `trailing space `, `aux.rs`, `NUL`, `CON` and `com1.txt`, and a file inside a `CON` folder, each read back with its contents through the decoded path;
  - a `mklink /J` junction;
  - `C:\PROGRA~1`;
  - drive-letter case.

```sh
git show 384b341~1:crates/lspkit-server/src/uri.rs > crates/lspkit-server/src/uri.rs   # or 384b341 for PR #50
cargo nextest run -p lspkit-server --test uri_windows
git checkout crates/lspkit-server/src/uri.rs
```

Failing rows, out of the table size:

| `uri.rs` | `uri_windows` tests | Round trips | Paths → URIs | Rejected paths accepted | Rejected URIs accepted |
|---|---|---|---|---|---|
| pre-fix `384b341~1` | 0 / 10 | 22 / 34 | 12 / 14 | 4 / 9 | 15 / 20 |
| PR #50 `384b341` | 4 / 10 | 12 / 34 | 4 / 14 | 4 / 9 | 8 / 20 |
| `7f37353` and later | 10 / 10 | 0 / 35 | 0 / 14 | 0 / 9 | 0 / 23 |

The per-row tallies were taken before the `%252F` round trip and the encoded-host rejections joined the tables.

The PR #50 rows that failed on Windows:

- `file://%3F/C:/a.rs` decoded to the verbatim path `\\?\C:\a.rs`, and `file://./C:/a.rs` to the device path `\\.\C:\a.rs`.
- `\\.\pipe\name`, `\\?\Volume{…}` and `\\?\GLOBALROOT\…` encoded to URIs.
- `%2F` and `%5C` inside a name became real separators.
- A URI with a port or userinfo decoded to a UNC server name.
- A lower-case drive letter or server name gave one file a second URI.
- A real file named `trailing dot.` decoded to a path Win32 opens as `trailing dot` (NotFound).
- A real file named `NUL` decoded to a path that opens the NUL device on Windows 11.

The full row-by-row output is on #49 (comment 6959676768).

Two more Windows-only failures came out of the same work:

- `tests/uri_authority.rs` did not compile on Windows (`-D unused-imports` once its only test was gated off).
- `tests/uri.rs` `truncated_percent_encoding_passes_through_bytes` failed there, because `/tmp/a%` has no drive.

### #52 on Windows

`pull_forget` 6/6 and `pull_forget_heap` 1/1 pass. `forget_frees_the_closed_documents_diagnostics_before_it_returns` holds a `Weak` to the forgotten report and was red on `d2a22c4`: `left: 1, right: 0`, because the memo stayed alive until a later pull and then until Salsa's next revision. It goes red again if either the immediate apply or the tombstone revision write is removed. `an_empty_uri_is_analyzed_never_mistaken_for_a_forgotten_slot` was red on `d2a22c4` with `left: [] right: [""]`.

### Windows-only regressions found by running CI's steps here

| Step | Failure on Windows | Fix |
|---|---|---|
| `make lint` | `clippy::doc_markdown` in a `cfg(windows)` doc comment in `lsp-profiler-dotnet/src/process.rs`. Linux never compiles it. | `13c6f82` |
| `make website-test` | `FileNotFoundError` from `subprocess.run(["npm", …])`: `CreateProcess` cannot find `npm.cmd` by its bare name. Pages were also read in the Windows code page. | `13c6f82` |
| `make test` | `clock::tests::century_leap_rules_hold_before_the_epoch` panicked with "overflow when subtracting duration from `SystemTime`". Windows `SystemTime` starts at 1601, so the 1600-02-29 row cannot be a `SystemTime` there, and fail-fast stopped the run. | `0dc2f04` |

### #42 on Windows

`sh scripts/release-assets.sh 0.0.0-test <dist> win-x64 linux-x64` produces both engine archives and the consumer kit. The kit holds no `bin/` or `obj/` output. With `LSP_PROFILER_DOTNET_ENGINE` pointing at the extracted `win-x64` `lsp-profiler-dotnet-engine.exe`, `cargo nextest run -p lsp-profiler-dotnet --all-features` passes 47 of 48, every .NET e2e session included. The one failure is `engine::tests::a_missing_engine_names_every_place_it_looked`, which checks the message for an absent engine and so needs the variable unset; it passes in `make test`.
