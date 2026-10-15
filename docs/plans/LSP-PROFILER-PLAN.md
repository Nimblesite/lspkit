# lsp-profiler Implementation Plan

**Spec:** [LSP-PROFILER-SPEC.md](../specs/LSP-PROFILER-SPEC.md)

**Goal:** one profiler, extracted from Basilisk and abstracted out of every consumer. Every language a runtime crate exists for — C#, F# and anything else on the CLR today; Python, Dart and Rust when their crates land — gets exactly the same CPU and memory profiling through the same Rust layer, the same wire and the same UI library. SharpLsp is the first consumer. Python is parked.

## Status legend

- `[x]` implemented and tested (the test is named or counted)
- `[~]` implemented, but tests are missing or incomplete, or not yet built
- `[ ]` not started

Status was last verified on **2026-10-15**.

## Phase 0: Core extraction

- [x] `crates/lsp-profiler` builds under the workspace lints. `cargo test -p lsp-profiler`: **74 passed**, 0 failed. `cargo clippy --all-targets`: clean.
- [x] Wire model `docs/models/profiler.td` and command registry `profiler-methods.json`, with the generator emitting Rust and TypeScript [PROF-WIRE-GEN].
- [x] Aggregation, speedscope / flame-graph / `.cpuprofile` exports, memory session scoring, leak confidence, timeline and `.heapprofile`, all unit-tested in the core.
- [x] `ProfilerService` dispatch, driven end to end with the real .NET backend (Phase 3's e2e suite).
- [x] `SamplerHandle::stop` drains batches while it waits for the backend's completion [PROF-SAMPLER]. A file backend that delivers its whole recording at stop (the .NET engine: ~1000 ticks for 3 s) deadlocked against the 256-slot channel before this; the CPU stop took the full 120 s finish timeout.
- [x] Spec written: every `PROF-*` ID cited in code resolves to exactly one heading (checked 2026-10-15).
- [~] Repository hygiene:
  - [x] `Makefile` (`build`, `test`, `clippy`, `fmt`, `dotnet`, `ui`, `ui-test`, `spec-check`, `check`).
  - [ ] `CLAUDE.md`. `AGENTS.md` imports `@CLAUDE.md`, which does not exist yet.
  - [ ] `README.md`.
  - [ ] `coverage-thresholds.json`, `deny.toml`, CI workflow.
  - [ ] `scripts/spec-citations.mjs` (the `spec-check` target names it; it is not written yet). The check was run by hand with a copy of SharpLsp's script.
- [ ] **Version control.** `lsp_profiler` is not a git repository, and no remote exists. Initialising one and pushing are the owner's call. SharpLsp consumes the library through `path:`/`file:` dependencies on the sibling checkout, which its CI cannot resolve (see Phase 5).

## Phase 1: LSP-native protocol [PROF-LSP]

- [x] Operations are `workspace/executeCommand` commands named `<prefix>.lspProfiler.<area>.<action>` (`crates/lsp-profiler/src/commands.rs`: `command_for`, `method_for`, `command_names`; 3 unit tests). The host advertises `command_names(prefix)` in `executeCommandProvider` and routes each command to `ProfilerService::handle`. The custom `lspProfiler/*` request names remain the internal method keys and the TypeScript `METHODS` table; no host exposes them as requests any more.
- [x] `experimental.lspProfiler` carries every runtime's `RuntimeInfo` [PROF-HOST-ROUTING]. The UI reads it from the client's initialize result to gate memory tracking per runtime mode (`ProfilerStore.memoryIsDirect`).
- [x] Cancellation [PROF-HOST-CANCEL]: the host answers `-32800` on `$/cancelRequest` while a command is in flight (SharpLsp e2e `an_in_flight_profiler_request_can_be_cancelled`). Every engine process is spawned with `kill_on_drop`.
- [ ] Positions become LSP `Location`s with zero-based lines [PROF-WIRE]. Still 1-based `file` + `line`; the UI library's adapter is the single place that reads them.
- [ ] Diagnostics [PROF-DIAGNOSTICS]: still delivered inside `ProfileReport` / `MemoryResult`; no service-side store, no `publishDiagnostics` merge, no `diagnostics_changed` event.
- [ ] Work-done progress [PROF-CPU-PROGRESS]: still the `lspProfiler/cpu/progress` notification; the host forwards it verbatim and the UI subscribes to it.
- [ ] `ProfilerService::{inlay_hints, code_lenses, code_actions}` [PROF-LSP-HINTS], [PROF-LSP-LENSES], [PROF-LSP-ACTIONS].
- [ ] `openArtifact` through `window/showDocument` [PROF-EXPORT]. The UI opens artifacts itself.
- [ ] Session cap in `ProfilerConfig` [PROF-CPU-START]. SharpLsp's `max_concurrent_sessions` setting is read but **not enforced** until this lands.
- [ ] Wire changes the .NET runtime needs: a neutral generation list on `HeapSnapshot` (today `gc_generations` carries gen0/gen1/gen2/large/pinned **bytes** for .NET and collection counts for Python); optional `GcResult` counts (today `collected` is 1/0 for "a collection was observed").
- [~] Host-boundary tests [PROF-TESTING]: advertised commands and cancellation are covered by SharpLsp's e2e and the UI library's mock-server suite; diagnostics merging and progress begin/end are not, because those features are not built.

## Phase 2: .NET engine [PROF-DOTNET-ENGINE]

`dotnet/LspProfiler.DotNet.Engine`, built with `make dotnet`.

- [x] Builds clean under `TreatWarningsAsErrors` and `AnalysisLevel=latest-recommended` (`dotnet build -c Release`).
- [x] `cpu`: EventPipe sample profiler, rundown, `TraceLog` symbolization, ticks streamed on stop. Validated against `dotnet/fixtures/HotLeakApp`: `HotLeakApp.Hot.Spin` resolves to `Program.cs` lines 29–32 on macOS; a hidden sequence point (line 0) is reported without a location.
- [x] **Design change: no dumps.** `createdump` hangs on this macOS for every target (also through `dotnet-dump collect`, which is what SharpLsp's old profiler used), so ClrMD and the dump path were deleted. Memory capture is the runtime's EventPipe heap snapshot ([PROF-DOTNET-HEAP-CAPTURE], [PROF-DOTNET-HEAP-GRAPH]), the same on every OS. Validated: 506 k live objects captured, `LeakedPayload` located at its declaration, growth between two snapshots, the retention path `LeakedPayload → LeakedPayload[] → List<LeakedPayload>` ending at the static root `Items`.
- [x] `gc`: `GCHeapCollect` does induce a full collection on .NET 10; the generation sizes after it are reported. Stopping a session from inside an event callback deadlocks the stream, so every session is stopped from the command thread (`GcWatch` + `SessionRun`).
- [x] `refs`, `objects`, `analyze` over the kept graph file; `failure` kinds for a dead PID (`ProcessNotFound`) and a non-.NET PID (`UnsupportedRuntime`).
- [x] `cpu`: a target that exits mid-session no longer fails the engine's stop (`StopRecording`).
- [ ] **Defect — samples lost when the target exits.** With no live target there is no rundown, so every managed frame stays unresolved (`!?`) and the session reports 0 samples; 748 ticks were recorded in the manual run. Fix: capture the method map at attach. Pinned by `a_target_that_exits_mid_session_keeps_its_samples` (fails) and SharpLsp's `profiler-cpu-sessions` suite.
- [ ] **Defect — no data before stop.** The engine symbolizes the trace only after `stop`, so `cpu.snapshot` mid-session reports nothing ([PROF-CPU-SNAPSHOT]) and `live_progress` is false. Fix: stream symbolized ticks while recording. Pinned by `a_mid_session_snapshot_reports_the_samples_so_far` (fails).
- [ ] Engine test project. The engine is covered through the Rust e2e suites, not a C# test project.
- [ ] Packaging: framework-dependent publish output, a size budget, and licence notices for the bundled packages. Today SharpLsp copies the `bin/Release/net8.0` directory beside its server binary.
- [ ] Windows and Linux validation of the heap snapshot and symbolization (only macOS was run).

## Phase 3: `lsp-profiler-dotnet` crate [PROF-DOTNET]

- [x] Scaffolded and a workspace member. `cargo test -p lsp-profiler-dotnet`: **15 unit tests** pass; **9 e2e tests**, 7 pass and 2 fail on the engine defects of Phase 2; clippy clean.
- [x] Service fixes pinned by the e2e suite: a PID naming no process is `PROCESS_NOT_FOUND` on `kill`, `cpu.start` and `memory.start` (it was `UNSUPPORTED_RUNTIME` on `kill`); `memory.start` recognises the PID before opening a session (a dead PID used to get a session); `cpu.start` reports the runtime version the classifier resolved when the backend reads none (it was empty); a `duration` caps the recorded time (the report used to time until `stop`).
- [x] [PROF-DOTNET-PROCESSES]: classification and the diagnostics-endpoint check moved here from SharpLsp (`process.rs`, 5 tests). SharpLsp's originals are deleted.
- [x] [PROF-DOTNET-ENGINE-LOCATE]: configured path, `LSP_PROFILER_DOTNET_ENGINE`, beside the host executable; `TOOL_MISSING` names every place looked.
- [x] `CpuBackend` (`cpu.rs`): spawns `cpu`, forwards frames and ticks with `weight_secs`, stops by writing a line to stdin, 120 s finish timeout, `live_progress: false`.
- [x] `MemoryBackend` direct mode (`memory.rs`): graphs under `<output_dir>/heap-<session>/`, deleted on stop; `diff` is a fresh snapshot the core compares.
- [x] e2e through `ProfilerService` against HotLeakApp (`tests/dotnet_e2e.rs`): the process is recognised and attachable; CPU finds `Hot.Spin` and its lines and writes all three artifacts; memory sees the leak's declaration line, growth, the static `Items` root, objects, an observed collection and a clean stop.

## Phase 4: `lsp-profiler-ui` [PROF-UI]

`packages/lsp-profiler-ui`, `@nimblesite/lsp-profiler-ui`.

- [x] Package scaffold: strict `tsconfig`, `tsc` build, `@vscode/test-cli` runner, `@preact/signals-core`.
- [x] Basilisk's extension UI ported: every file of the table below exists under `src/`, re-targeted at `ProfilerHost` + `ProfilerStore` instead of Basilisk's `Store`, with `<prefix>.profiler.*` command IDs, `<prefix>.profiler.*` context keys, `<prefix>.lspProfiler.*` server commands and the host's brand in every message [PROF-UI-IDS]. `wire-adapters.ts` is the one place that reads the wire's spelling.

  | Area | Files |
  |---|---|
  | CPU | `profiler`, `profiler-webview`, `profiler-flamegraph-html`, `profiler-decorations`, `profiler-state`, `profiler-status`, `profiler-styles`, `profiler-format`, `profiler-launch`, `profile-server` |
  | Memory | `memory-profiler`, `memory-capture`, `memory-dashboard`, `memory-dashboard-mapping`, `memory-decorations`, `memory-ref-graph`, `memory-ref-picker`, `memory-autopilot`, `memory-status` |
  | Processes | `process-explorer`, `process-explorer-rows`, `process-launch`, `process-poll`, `processes-state`, `process-reactivity` |
  | Debug courier | `dap-evaluate`, `dap-output`, `progress-ops` |
  | Shared | `host`, `store`, `ids`, `settings`, `wire-adapters`, `unknown-shape`, `reactive-refresh`, `timeouts`, `logger`, `index` |

- [x] Direct and courier memory modes: a `Completed` step presents immediately; an `Evaluate` step pauses the debuggee, evaluates and ingests [PROF-MEMORY-MODE]. Memory tracking on a processes row is offered for the debuggee of a courier runtime and for any attachable process of a direct runtime [PROF-UI-CAPABILITIES].
- [x] `scripts/contributions.mjs`: the package.json contributions (commands, view, welcome states, menus, keybindings, settings) generated for a host prefix, so a host's manifest and the library's registrations cannot drift.
- [x] Tests: **152 passing** in VS Code against a mock language server that speaks the wire (`src/test/host`, `src/test/suite`): command registration and advertisement, manifest contributions and welcome honesty, settings, status bars, reactive store and panel gating, processes panel rows/sort/group/filter/decorations/inline actions, webview CSP and theming, profile server, decorations and heat levels. These are Basilisk's `profiler`, `profiler-ux`, `profiler-entrypoints`, `profiler-panel-reactive`, `process-explorer`, `process-reactive-store`, `profiler-decorations`, `profiler-webviews` and `profile-server` suites, with their assertions kept.
- [x] Basilisk's server-backed suites (`profiler-cpu-e2e`, `memory-e2e`, `memory-introspection-e2e`, `memory-autopilot-e2e`, `profiler-memory-integration`, `memory-discoverability`) are ported into SharpLsp's extension suite against real .NET targets (see Phase 5). The Python-only flows (cooperative sampler injection, tracemalloc courier pauses, breakpoint-driven autopilot) have no direct-mode analogue and are not ported; interval autopilot is.
- [x] Product and language remnants removed, each pinned by a test (`host-identity`, `memory-runtime-neutral`, `process-explorer`): the webview headings read the host brand (they said BASILISK in SharpLsp); the courier markers are `__LSPPROF_*` like the backend (the UI waited for `__BASILISK_*`, so no courier round-trip could succeed); 50 citations of Basilisk spec IDs remapped and `spec-check` now rejects any foreign ID; rows use the runtime's `display_name` (not a capitalised ID); the ref picker offers the snapshot's own type names (it offered Python builtins and source-text class names that never match a .NET heap); the dashboard shows type names; the GC toast reports freed bytes (it read a runtime-defined counter as objects); a row opens its entry only when the runtime advertises `source_entries` (SharpLsp opened native binaries in the editor).
- [ ] The cooperative CPU path and the memory courier still recognise their results by console markers ([PROF-INVARIANTS] forbids scraping). Python is parked; replace before un-parking.
- [ ] Numeric line decorations → LSP inlay hints [PROF-UI-HEATMAP] (depends on Phase 1).
- [ ] SharpLsp's richer retention graph (nested expansion, filters, SVG/PNG export, browser harness) was **deleted with SharpLsp's old profiler, not merged** [PROF-UI-REFGRAPH]. Its sources are in SharpLsp's history (`src/editors/vscode/src/profiler-graph*.ts`, `profiler-graph-webview.mts`). Porting them onto the neutral `RefGraph` is open.
- [ ] Lint config for the package (SharpLsp's reactive-screen and no-legacy rules). `npm run lint` fails today: there is no `eslint.config.*`.
- [ ] Distribution: `file:` dependency today; publish to npm before any consumer releases.

## Phase 5: SharpLsp integration

Tracked in SharpLsp's plan: `SharpLsp/docs/plans/PROFILER-PLAN.md`, section "Migration to lsp-profiler". State on 2026-10-15:

- [x] Rust host mounted (`src/sharplsp/src/profiler/host.rs`): commands advertised, `experimental.lspProfiler`, routing, cancellation, notifications, shutdown before sidecars. Old profiler modules (4,949 LOC) deleted. 205 unit tests and the new `profiler_shared` e2e module (5 tests, real `ProfileTarget`) pass; clippy clean.
- [x] VS Code extension rewired onto the library (`src/profiler-host.ts`), old profiler sources and suites deleted, contributions generated by `tools/vsix/sync-profiler-contributions.mjs`, engine staged beside the server binary by the Makefile.
- [x] Extension suites against live .NET targets: `profiler`, `profiler-cpu-e2e`, `memory-e2e`, `profiler-processes`, `profiler-cpu-sessions`, `memory-flows`. The `profiler` chunk: **82 passing, 2 failing** on macOS, the two failures being the engine defects of Phase 2.
- [ ] A distribution SharpLsp's CI can resolve (needs Phase 0's version-control decision).
- [ ] Parity gaps, decided and specified before a release: live counters (`metrics`), opening a recorded `.nettrace` (`import`), object field inspection (`inspect`), keeping a dump for external tools, the retention-graph features above.

## Parked

- **Python runtime** (`crates/lsp-profiler-python`, `crates/lsp-profiler-helper`): not workspace members, no `Cargo.toml` for the python crate; 30 legacy `PROFILE-*` citations to rename; must adopt the stop-drain and the executeCommand mapping.
- **Basilisk.** Its in-tree profiler, handlers, tests and VSIX were moved out of its working tree. **Basilisk has no profiler until it is rewired** onto this library and the Python runtime.
- **Dart, Rust/native runtimes.** Not specified [PROF-SCOPE]. Each is one crate implementing `Runtime`; nothing above the trait changes.

## Risks and open decisions

| Item | Risk or decision | Resolution path |
|---|---|---|
| TraceEvent source lines on Linux and Windows | Validated on macOS only | Run `make dotnet && cargo test -p lsp-profiler-dotnet` on both |
| Heap graph size | ~16 MB JSON per 500 k objects, kept per snapshot | Keep only the latest graph, or compress; measure a 1 GB heap |
| No live CPU sample count for .NET | EventPipe delivers samples after stop | Accepted (`live_progress: false`); the panel shows "Profiling PID" without a counter. Streaming `EventPipeEventSource` is a later improvement |
| Command-ID collision in VS Code | LSP clients auto-register server commands | Server commands use `<prefix>.lspProfiler.*`, UI commands `<prefix>.profiler.*` (done) |
| Diagnostics replacement | `publishDiagnostics` replaces a URI's whole set | Host-side merge [PROF-HOST-DIAGNOSTICS] when Phase 1 diagnostics land |
| Session cap | SharpLsp's setting is unenforced | Phase 1 item |
