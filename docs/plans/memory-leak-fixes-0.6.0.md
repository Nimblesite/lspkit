# Memory leak repairs after v0.6.0

## [MEM-060-SCOPE] Scope

Fix every leak in the v0.6.0 static review (four pi agents on GLM 5.3 plus a manual pass), following [fix-bug](../../.agents/skills/fix-bug/SKILL.md). Each fix has a regression test that failed because of the leak before the fix: a `Weak`/`strong_count` check, a bounded count of retained entries, or a released file handle. None uses RSS, timing or `sleep`. The rules are the ones in [AGENTS.md](../../AGENTS.md#memory-lifetimes-).

## [MEM-060-CHECKLIST] Fixes

| ID | Finding | Fix | Regression test |
| --- | --- | --- | --- |
| [MEM-060-PARSE-CACHE] | H1 `ParseCache` kept one tree per edit | FIFO bound of `MAX_TREES` (128) | `lspkit/tests/treesitter_cache.rs` |
| [PROF-CPU-SESSIONS] | H2 ended CPU sessions stayed forever | A closed sampler counts as ended; all but 8 ended sessions are evicted on register | `lsp-profiler/src/session_lifetime_tests.rs` |
| [PROF-UI-LIFETIME] | H3 DAP output, stop-state and pending resumes kept per ended session | Evicted on `onDidTerminateDebugSession` | `lsp-profiler-ui/src/test/unit/dap-output-eviction.test.ts` |
| [PROF-CPU-PROGRESS] | M1 progress task held the whole service | Weak back-reference, upgraded per tick | `lsp-profiler/src/service/lifetime_tests.rs` |
| [PROF-MEMORY-SESSION] | M2 evicted and shut-down memory sessions kept backend state (.NET heap graphs) | Eviction and `stop_all` (called by `shutdown`) run the backend's `stop` | `lsp-profiler/src/memory/session_lifetime_tests.rs` |
| [MEM-060-EXAMPLE-TASKS] | M3 example detached request tasks pinned a `Server` | `Tasks` owns a `JoinSet`, reaped per spawn and aborted at session end | `examples/diagnostic-reference/session/task_retention.rs` |
| [MEM-060-PULL-ADMIT] | M4 pulls for URIs outside the session admitted Salsa inputs nothing forgot | Forget a URI that is neither open nor a JSON file in the watched root, after pulls, scans and watched-file changes | `examples/diagnostic-reference/session/admit_retention.rs` |
| [MEM-060-QUEUED-REQUEST] | M5 a request registered but never prepared had no owner | `CancellationHandle::queue` returns a `QueuedRequest` guard; the example holds it until `prepare` | `lspkit-server/tests/dispatch_memory_retention.rs` |
| [MEM-060-SIDECAR-NOTIFICATIONS] | M6 unread sidecar notifications queued without bound | Bounded at `NOTIFICATION_CAPACITY`, overflow counted | `lspkit/tests/sidecar_notification_bound.rs` |
| [MEM-060-SCHEDULER-TASK] | M6 the live scheduler outlived a consumer that dropped its triggers | The loop selects on `closed()` | `lspkit-server/tests/live_scheduler.rs` |
| [MEM-060-EXAMPLE-OUTBOX] | M6 workspace-change notifications queued behind a stalled editor | One queued notification covers every change until the pump dequeues it | `examples/diagnostic-reference/session/outbox_retention.rs` |
| [PROF-UI-LIFETIME] | M7 `ProfilerUi.dispose()` left the processes provider and tree view alive | `registerProcessesView` returns both among its disposables | `lsp-profiler-ui/src/test/teardown/ui-teardown.test.ts` (own editor instance) |
| [PROF-UI-LIFETIME] | M8 capture timeline and autopilot ledger grew per capture | `appendBounded`, 120 entries, matching the core timeline | `lsp-profiler-ui/src/test/unit/memory-timeline-bound.test.ts` |
| [MEM-060-DIAGNOSTICS-SINK] | LOW `DiagnosticsBus::attach` had no detach | `attach_owned` returns a `SinkRegistration` guard | `lspkit-server/tests/diagnostics.rs` |
| [PROF-EXPORT] | LOW profiler artifacts on disk were never pruned | Keep the newest 32 per extension after each write | `lsp-profiler/src/export/artifact_retention_tests.rs` |
| [PROF-UI-LIFETIME] | LOW `FileLogSink` had no dispose and no size cap | `dispose()` closes it; two alternating capped files (8 MiB each) | `lsp-profiler-ui/src/test/unit/file-log-sink.test.ts` |
| [PROF-DOTNET-CPU] | LOW `cpu.rs` forwarder is spawned unowned | Already ends with its consumer: `SamplerHandle::drop` signals stop. The test pins it | `lsp-profiler-dotnet/src/cpu.rs` tests |

## [MEM-060-NOT-LEAKS] Checked and left as they are

- **M9 `Symbolizer` `_ids`:** each CPU session runs its own engine process, and the interner is that recording's frame dictionary, mirrored by the Rust relay. It goes when the session's engine exits.
- **Other unowned handles (`live/watcher.rs`, `cooperative.rs`):** each ends with its consumer. The watcher thread exits when its receiver or native watcher drops; the cooperative tail thread exits at its header deadline or when its sink is dropped.
- **`forgotten` close queue:** a `HashSet` of URIs, bounded by distinct closed documents and drained by the query that holds the state once it finishes.
- **Decoration-type maps:** keyed by palette colours (a handful of heat and confidence levels) and disposed on deactivate.
- **Sidecar responses and the example's response frames:** bounded by the requests in flight. Dropping a response would break the protocol.
- **One analysis per trigger:** `Session::run` keeps its tested contract (`run_advances_generation_on_each_success`). The scheduler adds no memory: each event lands in exactly one trigger.

## [MEM-060-UNTESTABLE] Not fixed

`lsp-profiler-python` is parked: it is excluded from the workspace, depends on `basilisk-common`, which is not in this repository, and does not build. No test can compile against `stop_tracemalloc` (`gc.garbage` from `DEBUG_SAVEALL`, the baseline snapshot, the signal and `atexit` hooks), `pyspy.rs` or the helper's `main.rs`, so per fix-bug they are not changed. They need fixing when the port lands.

## [MEM-060-CONTRACT] API changes

- `lspkit::sidecar::inbound::Inbound::notifications` is a bounded `mpsc::Receiver`. `Inbound` gains `dropped_notifications`.
- `lspkit-server` adds `CancellationHandle::queue`, `QueuedRequest`, `DiagnosticsBus::attach_owned` and `SinkRegistration`.
- `lsp-profiler` adds `MemorySessionManager::stop_all`, `export::MAX_ARTIFACTS_PER_KIND` and `export::ARTIFACT_PREFIX`. `ProfilerService::shutdown` now also stops memory sessions.
- `@nimblesite/lsp-profiler-ui`: `FileLogSink` takes an optional byte cap and gains `dispose(): Promise<void>`.
