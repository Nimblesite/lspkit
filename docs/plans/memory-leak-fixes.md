# Memory leak repairs — 2026-11-16

## [MEM-FIX-SCOPE] Scope

Repair the five allocation-lifetime defects proved by the [audit](memory-leak-audit.md), plus the initialized watcher leak discovered during validation, following [fix-bug](../../.agents/skills/fix-bug/SKILL.md). Preserve all original leak proofs unchanged. Add direct allocator measurements for the correlator and dispatcher before repairing them; a count-only sweep must not substitute for freeing memory when work ends.

## [MEM-FIX-CHECKLIST] Required steps

- [x] Read the skill, source, specifications, and original evidence.
- [x] Reconfirm all five original failures and record direct heap-retention failures.
- [x] Repair abandoned correlator request ownership; prove release.
- [x] Repair inbound reader shutdown; prove decoder release.
- [x] Release ingress registrations on terminal preparation errors; prove release.
- [x] Release closed reference-server document buffers; prove release.
- [x] Break the reference-server handler cycle; prove release.
- [x] Prove and repair initialized watcher retention after session teardown.
- [x] Run `make test`, `make lint`, formatting, and Deslop without weakening gates.
- [x] Record pi review results and exact validation evidence.
- [ ] Clean owned temporary/build files and processes; check disk space.
- [ ] Open and merge a green PR, publish v0.5.0, verify registries and notify consumer agents.

## [MEM-FIX-RESULTS] Validation

All eight leak tests pass after failing for actual retained allocations before the fixes. Full `make ci` passes: 496 Rust tests, all ten coverage floors, TypeScript tests and coverage, static analysis, and the release build. `make fmt CHECK=1` passes. All three original proof files remain byte-identical. Exact output: [memory-leak-fix-evidence.md](memory-leak-fix-evidence.md).

| Ownership defect | Executed failure before repair | Repair |
| --- | --- | --- |
| Abandoned correlator responses | 32 response channels retain 2,099,200 heap bytes; zero deallocations | `PendingResponse` drop removes the sender immediately, with a weak registry reference |
| Detached inbound readers | 32 decoder allocations remain live after every receiver owner drops and the runtime parks | The reader selects channel closure alongside incoming frames; both destinations must close |
| Rejected preregistered requests | 32 terminal errors retain 2,105,568 heap bytes | Unknown-method preparation adopts and drops the ingress registration guard |
| Closed reference documents | 32 closed documents retain 2,097,152 source bytes | Handle `didClose`, remove the source buffer and invalidate diagnostics |
| Reference handler cycle | Source allocation retains four strong owners after the server drops | Handler captures exclude their owning dispatcher |
| Initialized reference watcher | Real source allocation retains one strong owner after initialized session drops | Session owns and aborts the watcher task; callback holds only weak session ownership |

The allocator tests warm map capacity, then compare actual allocated and freed bytes before registry inspection, shutdown, or owner teardown. Weak probes establish whether specific real allocations remain alive; these tests do not infer leaks from RSS, elapsed time, or generic errors. Isolation is needed because exiting an LSP process would mask all retained memory.

## [MEM-FIX-CONTRACT] API and consumer impact

The only public signature change is `Correlator::register` returning `PendingResponse<R>` instead of a bare Tokio receiver. Await and `try_recv` remain supported. Under [SIDECAR-REQUEST-LIFETIME](../specs/lspkit-spec.md#cross-cutting-helpers--required-behavior), an abandoned ID returns `UnknownId`; preserving the former `AlreadyResolved` outcome after deterministic abandonment would require retaining an unbounded history. The old API assertion is updated to the explicit new contract and additionally asserts immediate registry release. Every original memory-leak proof remains unchanged. `AlreadyResolved` still reports the concurrent drop/send race.

Inbound and dispatcher signatures are unchanged. Three fixes are in the reference server example, not consumers' own document stores or handler registrations. No PullStore eviction API or watcher payload feature is part of this patch. These tests do not establish the causes of previous consumer incidents. The consumer agents have received scope and API details through TMC. The user authorized a PR and release; v0.5.0 is the target because the correlator return type changes.

The first full run exposed a separate existing harness failure: Cargo placed tests below `build/<crate>/<hash>/out`, so inferring the producer path from two parent directories failed before real sessions started. `scripts/rust-coverage.sh` now obtains the executable from Cargo's parsed artifact metadata, overriding stale caller settings with the just-built instrumented producer. No test assertion or coverage gate changed.

Coverage gaps are covered by deterministic receiver-owner teardown, closed-channel and poisoned-registry tests, native watcher observation loss and consumer shutdown, absent native parse results, and real .NET capability/error-path negotiation. Native failure paths require isolation because safe public handles cannot deterministically force OS overflow or a missing tree. Existing coverage thresholds and reporting semantics remain unchanged. The pi coverage review incorrectly attributed the gaps to stale profiles; a clean rebuild reproduced them, so that recommendation was rejected and actual missing paths were tested.
