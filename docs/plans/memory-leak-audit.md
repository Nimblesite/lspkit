# Memory leak audit — 2026-11-16

## [MEM-AUDIT-SCOPE] Outcome and proof standard

**Five confirmed ownership leaks: three in shared library APIs, two in the actual reference LSP implementation.** Nine new tests execute successfully as binaries: five fail on retained allocations and four controls pass. [Complete failure outputs and reproduction commands](memory-leak-audit-evidence.md).

This is the requested audit and failing-test deliverable. Production behavior is unchanged; the only addition to an existing implementation file is a `cfg(test)` module declaration. The regression tests deliberately remain red, with no ignores, expected-panic attributes, weakened assertions, or changed gates. The [fix-bug workflow](../../.agents/skills/fix-bug/SKILL.md) has its failing-test prerequisite ready for each repair.

A leak claim requires allocations still owned after the relevant operation ends. Tests use live registry entries owning concrete heap storage, `Weak::strong_count`, and retained `String` lengths. None uses RSS growth, arbitrary sleeps, compilation failures, or fabricated protocol responses as proof. Small isolation tests are necessary here: exiting an LSP child would let the OS reclaim its memory and hide the very ownership defect under examination.

## [MEM-AUDIT-CHECKLIST] Mandatory checks

- [x] Read repository rules, relevant specifications, test-first workflow, and PiNotes model guidance.
- [x] Select installed current pi models and run bounded Rust, profiler/.NET, UI, and consumer-trace reviews.
- [x] Independently inspect candidates, lifecycle implementations, and existing tests.
- [x] Execute deterministic failing tests for every confirmed leak before changing production behavior.
- [x] Record exact failures, ownership chains, controls, and unproven/rejected candidates.
- [x] Run static analysis, existing regressions, and Deslop without weakening gates.
- [x] Record verified model results in PiNotes.
- [ ] Remove audit temporary files and unnecessary owned build output, stop owned processes, and check disk space.

## [MEM-AUDIT-CORRELATOR] Abandoned requests retain channel allocations

**Shared library:** [sidecar/correlator.rs](../../crates/lspkit/src/sidecar/correlator.rs), `Correlator::register`, `pending`, and `resolve`.

`Correlator.pending → oneshot::Sender<R> → Arc<Inner<R>> → inline Option<R>` remains owned after the receiver is dropped. Only a response removes the entry; there is no abandonment or disconnection cleanup. Timeouts/cancellation followed by a backend that never replies accumulate entries for the correlator's lifetime.

[Proof](../../crates/lspkit/tests/sidecar_memory_retention.rs): `abandoned_requests_release_their_channel_allocations` registers and drops 32 receivers with a 65,536-byte inline response type. It fails with **32 retained channel allocations, at least 2,097,152 bytes of response storage**, expected zero. The byte figure is the response slots alone, excluding channel/map overhead; it does not imply ordinary callers always use this payload size. The completed-response control runs 32 exchanges and returns to zero each time.

An initial waker-based hypothesis was rejected: Tokio released the waker allocations, while the channel slots remained. The final proof measures the retained slots, not that disproved hypothesis.

## [MEM-AUDIT-INBOUND] Dropped readers leave detached tasks owning decoder state

**Shared library:** [sidecar/inbound.rs](../../crates/lspkit/src/sidecar/inbound.rs), `Inbound.task` and `read_loop`; violates [LSP-PULL-TRANSPORT](../specs/LSP-PULL-SPEC.md#lsp-pull-transport-idle-compiler-events).

Dropping `Inbound` drops a Tokio `JoinHandle`, which detaches its task. The pending read still owns its reader, channels, decoder, and notification classifier. A peer that stays open and silent keeps that state indefinitely. Failed sends on notification-only traffic are also ignored.

[Proof](../../crates/lspkit/tests/sidecar_memory_retention.rs): `dropped_inbound_readers_release_their_decoder_allocations` drives 32 real framed exchanges through Tokio streams. Each decoder exclusively owns a 64 KiB `Arc<Vec<u8>>`; the test retains only weak observers and peer writers. After dropping all `Inbound` owners, the current-thread runtime's `on_thread_park` event proves the ready queue has drained. **All 32 decoder allocations remain alive (2 MiB)**, expected zero. Destroying the runtime releases every probe, positively identifying the task as the retaining owner. No wall-clock delay or guessed yield count is used.

## [MEM-AUDIT-DISPATCH] Terminal preparation errors retain ingress registrations

**Shared library:** [dispatch.rs](../../crates/lspkit-server/src/dispatch.rs), `Dispatcher::prepare`, and [dispatch/cancellation.rs](../../crates/lspkit-server/src/dispatch/cancellation.rs), `CancellationHandle::register`; violates [LSP-PULL-CANCELLATION](../specs/LSP-PULL-SPEC.md#lsp-pull-cancellation-request-lifetime).

Ingress registration stores `RequestControl` and its owned interest strings before dispatch. An unknown-method error returns before creating `RegistrationGuard`. Neither that error nor `cancel_all()` removes the terminal registration. Plain `prepare` without ingress registration does not have this defect.

[Proof](../../crates/lspkit-server/tests/dispatch_memory_retention.rs): `unknown_method_releases_pre_registered_interest` creates 32 ingress controls, each with its own 64 KiB input string, observes all 32 terminal errors, and cancels everything. The registry still reports **`in_flight: 32` instead of zero**, retaining at least 2 MiB of input strings. Separate controls pre-register the same 32 requests and release all entries after completion or after dropping unpolled prepared futures.

## [MEM-AUDIT-CLOSED-DOCUMENTS] Reference LSP ignores document closure

**Reference implementation, not a generic VFS defect:** [diagnostic-reference/session.rs](../../crates/lspkit-server/examples/diagnostic-reference/session.rs), `handle_notification` and `store_document`.

The notification handler inserts full document text for `didOpen`/`didChange` but never handles `textDocument/didClose`. The source map therefore owns every closed document's last text for the session lifetime.

[Proof](../../crates/lspkit-server/examples/diagnostic-reference/session/memory_retention.rs): `closed_documents_release_their_source_buffers` invokes the actual handler with 32 open/close sequences, each containing 64 KiB of text. After all close notifications, the actual source map retains **32 documents and 2,097,152 text bytes**, expected `(0, 0)`. Handler registration is unnecessary to reproduce this defect, so it is independent of the cycle below.

## [MEM-AUDIT-HANDLER-CYCLE] Reference LSP handlers retain their own dispatcher

**Reference implementation:** [diagnostic-reference/session.rs](../../crates/lspkit-server/examples/diagnostic-reference/session.rs), `register_handlers`.

`Dispatcher.handlers → handler closure → cloned Server → Dispatcher.handlers` forms a strong `Arc` cycle. Those captured servers also retain source buffers, the pull database, gate map, and output queue. Dropping every external server owner cannot break it.

[Proof](../../crates/lspkit-server/examples/diagnostic-reference/session/memory_retention.rs): `registered_session_releases_its_source_allocations` opens a 64 KiB document, calls the actual registration function, takes a weak observer of the source registry, and drops the server. **Four strong owners remain**, expected zero. The otherwise identical unregistered-session control releases the registry completely. This is a genuine unreachable cycle, not a live cache.

## [MEM-AUDIT-CONSUMERS] Consumer impact and remaining candidates

These tests establish defects in this checkout. **They do not establish that these five defects caused the previously observed memory spikes in Osprey, SharpLsp, or Deslop.** The bounded consumer trace confirmed SharpLsp's use of shared `PullStore` and ingress `CancellationHandle::register` in `src/sharplsp/src/pull_diagnostics/requests.rs` and `src/sharplsp/src/inbound.rs`; it did not reproduce a consumer memory incident. No consumer repository was modified.

| Candidate | Evidence and limit |
| --- | --- |
| `PullStore` retains URI inputs and Salsa diagnostic memos without a document-removal API | High-priority unbounded-retention risk, especially on solution/workspace churn. SharpLsp uses this store. Memoization is explicitly part of the spec; no executed lifecycle test yet establishes unintended retention after a supported eviction boundary. Not counted as a proven leak. |
| Refused ingress requests never adopted by `prepare_call` | Shared registry has no explicit discard operation. SharpLsp preregisters before foreground dispatch; rejected-after-shutdown traffic is a candidate trigger. The test above proves the terminal unknown-method path, not SharpLsp's shutdown path. |
| UI `memory-profiler.ts` keeps `boundStore` after disposal; host logger/palette bindings also retain their last host | Static ownership candidates, not GC-tested. A functional accessor returning old state alone is insufficient proof of retained allocations. |
| Concurrent `profile-server.ts::ensureServer` calls can each create a loopback server before the singleton is adopted | Independently observed race candidate; only the last server is stored for disposal. Requires an executed real-host test of surviving server handles before being called a confirmed leak. |
| Memory-manager eviction removes only its map entry | .NET backend has a separate session map and heap-graph directory. The evicted ID becomes unavailable to normal stop routing. Not reproduced through the real engine; file retention is not automatically a RAM-leak proof. |
| Profiler shutdown only calls CPU `stop_all` | Memory backend cleanup is a follow-up; the large heap graphs identified by pi live on disk, not in the Rust session map. |
| Hung .NET CPU forwarder | Its detached task can wait indefinitely on engine reads/process completion even after the consumer's bounded stop. Conditional candidate; no hung real-engine proof executed. |
| .NET capture exception cleanup | Candidate temporary-file/handle retention requires an actual exception reproduction. Not counted as a memory leak. |
| Reference output pump owns an `Outbox` sender to its own receiver | Additional task-lifetime candidate at runtime scope; not independently tested and not included in the five confirmed findings. |

Reviewed cleanup paths with no confirmed leak: VFS `close` removes documents; explicit parse-cache `clear` releases its caller-owned cache; combined watchers abort their forwarding task; normal dispatcher futures have a release guard; sampler drop signals stop; memory timelines are bounded and growth streaks prune absent sites; cooperative/helper process shutdown has explicit stop paths. CPU recordings retained until explicit stop are documented archival state. Diagnostics sink registration and parse-cache growth need caller-lifetime analysis, not a blanket leak label.

The parked Python runtime received a targeted lifecycle inspection, not an exhaustive executable audit. UI/.NET candidates remain unproven. This scan is not a guarantee that the repository or its consumers contain no further leaks.

## [MEM-AUDIT-VALIDATION] Executed validation

- **New proofs:** 5 ownership assertions fail, 4 release controls pass; exact commands and complete output are in [the evidence file](memory-leak-audit-evidence.md).
- **Existing nearby suites:** 9 correlator tests and 12 dispatcher tests pass, demonstrating why existing green checks missed these cases.
- **`make lint`: exit 0.** Workspace release Clippy with warnings denied, spec-citation checks, skill-link checks, TypeScript 7/oxlint, and .NET builds pass. Cargo separately emits existing unused-manifest-dependency warnings; .NET reports zero warnings/errors.
- **Deslop:** no connected Deslop MCP tools were available, so the documented [CLI fallback](https://deslop.live/docs/for-ai/) was used. Read the existing `0.0.0-dev` report before writing; then ran the installed bundled CLI and inspected the JSON. Final report has no cluster touching any new `memory_retention` test. Global duplication is 7.005%; thresholds/configuration unchanged. The repeated setup introduced by the pi draft was consolidated.
- **No full-suite green claim:** the requested leak regressions intentionally fail. No production repairs were made, so the fix skill's post-repair full test run is not represented as completed.
- **Pi verification:** Rust, profiler/.NET, UI, and consumer reviews used `zai/glm-5.3`. The test draft used `zai/glm-5.3-flash`; its first control incorrectly left 31 deliberately queued requests and violated several lints. It was corrected before acceptance, and all final assertions were independently rerun. `kimi-coding/k3.1-pro-highspeed` returned a quota error and contributed no review. Missing APIs/compile failures suggested by reviewers were rejected as leak proofs. Details: `PiNotes/2026-11-16-lspkit-memory-audit.md`.

## [MEM-AUDIT-CLEANUP] Cleanup

Pending final cleanup. No git commands, commits, pushes, issue changes, cloud jobs, or VS Code process termination were performed.
