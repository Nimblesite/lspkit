# Review: v0.5.0 → fix/issues-21-26-48-49 (2b2a2b4)

Scope: `git diff v0.5.0..2b2a2b4`, 31 files, +1335/−75. The production code under review is `uri.rs` (#49, PR #50), `outbound.rs` and `jsonrpc.rs` (#48), `correlator.rs`, and `clock.rs` (which replaces humantime). The rmcp and humantime removals, the `mcp` feature removal and the toolchain pin are also covered.

## Findings, most severe first

- [ ] **URI-DECODE-SEPARATOR** `crates/lspkit-server/src/uri.rs:70-81`: the URI is percent-decoded before it is split into segments. `%2F` and `%5C` therefore become real separators: `file:///C:/a%5Cb.rs` decodes to `C:\a\b.rs`, and on Unix `file:///tmp/a%2Fb` decodes to `/tmp/a/b`. The existing tests `lowercase_hex_percent_decodes` and `uppercase_hex_percent_decodes` (`tests/uri.rs:91-102`) assert this behaviour, but the #49 Windows plan requires the opposite: an encoded separator never becomes one. Fix: reject a decoded `/` or `\` inside a segment with `UriError::NoLocalPath`, and replace those two tests with rejection tests (a deliberate behaviour change, recorded in the spec).
- [ ] **URI-HOST-INJECTION** `uri.rs:71,79`: the authority is percent-decoded and then pasted into a UNC prefix, so `file://host%5Cshare%5Cx/a.rs` becomes `\\host\share\x\a.rs`. A host containing `/`, `\`, `?` or `:` after decoding must be rejected.
- [ ] **UNWRAP-OR** `uri.rs:70,88,124,140`: four `unwrap_or`/`unwrap_or_else` calls in new production code, which the user has banned. Replace them with `split_once`/`match`.
- [ ] **URI-NON-ROUNDTRIP** `uri.rs:117-140`: `path_to_uri` returns `Ok` for Windows paths that `uri_to_path` then rejects: `\\server` (no share) gives `file://server`, and `\\?\Volume{GUID}\x` gives `file:///Volume%7B…`. `path_to_uri` should return `InvalidPath` for both rather than emit a URI that never decodes.
- [ ] **BREAKING-MCP-FEATURE** `crates/lspkit-server/Cargo.toml`: removing the `mcp` feature breaks `cargo add lspkit-server --features mcp` for existing consumers with "unknown feature". This was intentional (rmcp was unused and pulled the unmaintained chrono), but the v0.6.0 release notes must say so.
- [ ] **BREAKING-PROFILER-PATH** `crates/lsp-profiler`: `lsp_profiler::session::rfc3339_now` moved to `lsp_profiler::clock::rfc3339_now`. No in-repo consumer uses it; release notes.
- [ ] **TIMING-TESTS** `crates/lsp-profiler-dotnet/tests/dotnet_e2e.rs:270,296`: both tests assert `elapsed < 5s` and fail under load (5.5 s seen at load average 10), which breaks the repo's "Deterministic, no timing" rule. This predates the diff but blocks local `make ci` on a busy machine.

## Verified sound

- `Outbound`: a registration released on `IdsExhausted` (`PendingResponse` drop), unknown/duplicate/foreign ids rejected visibly, heap proven by `outbound_heap_leak.rs`, routing proven by the 1↔2 swap mutant.
- `wire_params`: null `params` omitted per JSON-RPC 2.0; `"result": null` is still emitted for responses.
- `clock.rs`: Hinnant civil-from-days, floor before the epoch, century leap rows (1600/1900/2000/2100/2400), mutants killed (DeslopClaude1, SharpLsp1).
- Dependency removals: `cargo deny` and `osv-scanner` clean; rmcp was never called.
