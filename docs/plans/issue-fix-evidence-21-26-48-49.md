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
| #49 URI host read as a path folder; UNC and `\\?\` paths | `384b341` (PR #50) | not yet; ships in v0.6.0 | `crates/lspkit-server/tests/uri_authority.rs` `localhost_authority_is_the_host_not_a_folder`; Windows rows in `crates/lspkit-server/src/uri_style_tests.rs:46` and `:57` |
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

Red: remove the fix and keep the tests.

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
