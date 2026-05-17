---
name: code-dedup
description: Searches for duplicate code, duplicate tests, and dead code, then safely merges or removes them. Use when the user says "deduplicate", "find duplicates", "remove dead code", "DRY up", or "code dedup". Requires test coverage — refuses to touch untested code.
---
<!-- agent-pmo:74cf183 -->

# Code Dedup

Carefully search for duplicate code, duplicate tests, and dead code across the repo. Merge duplicates and delete dead code — but only when test coverage proves the change is safe.

## Prerequisites — hard gate

Before touching ANY code, verify these conditions. If any fail, stop and report why.

1. Run `make test` — all tests must pass. If tests fail, stop. Do not dedup a broken codebase.
2. `make test` is fail-fast AND enforces the coverage threshold from `coverage-thresholds.json`. If it exits non-zero, stop and fix it before deduping.
3. Rust is typed by default — proceed. (Workspace lints in `Cargo.toml` deny `unsafe_code`, `missing_docs`, and the clippy `unwrap_used` / `expect_used` / `panic` / `todo` / `unimplemented` / `unreachable` family — these guard rails are part of why dedup is safe here. Verify they are still active before starting.)

## Steps

Copy this checklist and track progress:

```
Dedup Progress:
- [ ] Step 1: Prerequisites passed (tests green, coverage met, workspace lints active)
- [ ] Step 2: Dead code scan complete
- [ ] Step 3: Duplicate code scan complete
- [ ] Step 4: Duplicate test scan complete
- [ ] Step 5: Changes applied
- [ ] Step 6: Verification passed (tests green, coverage stable)
```

### Step 1 — Inventory test coverage

Before deciding what to touch, understand what is tested.

1. Run `make test` to confirm green baseline. It exits non-zero on any test failure OR coverage shortfall (REPO-STANDARDS-SPEC [TEST-RULES], [COVERAGE-THRESHOLDS-JSON]).
2. Note the current line coverage percentage from `_coverage_check` output — this is the floor. It must not drop.
3. Identify which crates / modules have coverage and which do not. Only files WITH coverage are candidates for dedup.

### Step 2 — Scan for dead code

Search for code that is never called, never imported, never referenced.

1. The Rust compiler already warns on dead code under the workspace lints (`dead_code = "deny"`, `unused_imports = "deny"`, `unused_variables = "deny"`). Run `make lint` and read every warning that survived denial — they will be hard errors.
2. For Rust public items (`pub fn`, `pub struct`, re-exports) the compiler will NOT flag them as dead. Run `cargo +nightly udeps` or grep the workspace for each `pub` item with zero call sites (including `tests/`, `examples/`, doc-tests, and downstream consumer references where known).
3. For each candidate: **grep the entire codebase** for references (including tests, doc tests in `///` comments, build scripts, configs). Only mark as dead if truly zero references.
4. List all dead code found with file paths and line numbers. Do NOT delete yet.

### Step 3 — Scan for duplicate code

Search for code blocks that do the same thing in multiple places.

1. Look for functions/methods with identical or near-identical logic.
2. Look for copy-pasted blocks (same structure, maybe different variable names).
3. Look for multiple implementations of the same algorithm or pattern.
4. Check across crate boundaries — duplicates often hide between `lspkit-server`, `lspkit-mcp`, `lspkit-live`, `lspkit-vfs`, etc. The `EngineApi` trait surface in `lspkit` is the canonical place for shared engine logic.
5. For each duplicate pair: note both locations, what they do, and how they differ (if at all).
6. List all duplicates found. Do NOT merge yet.

### Step 4 — Scan for duplicate tests

Search for tests that verify the same behavior.

1. Look for `#[test]` functions with identical assertions against the same code paths.
2. Look for test fixtures/helpers duplicated across `tests/` directories of different crates.
3. Look for integration tests that fully cover what a unit test (`mod tests { ... }`) also covers — keep the integration test per CLAUDE.md ("Coarse E2E only").
4. List all duplicate tests found. Do NOT delete yet.

### Step 5 — Apply changes (one at a time)

For each change, follow this cycle: **change → test → verify coverage → continue or revert**.

#### 5a. Remove dead code
- Delete dead code identified in Step 2.
- After each deletion: run `make test` (fail-fast + coverage + threshold all in one).
- If `make test` exits non-zero (test failure OR coverage drop): **revert immediately** and investigate.
- Dead code removal should never break tests or drop coverage.

#### 5b. Merge duplicate code
- For each duplicate pair: extract the shared logic into a single function/module — usually the `lspkit` façade crate, but lower-level helpers belong with the crate that owns the domain (`lspkit-vfs` for buffer ops, etc.).
- Update all call sites to use the shared version.
- After each merge: run `make test`.
- If tests fail: **revert immediately**. The duplicates may have subtle differences you missed.
- If coverage drops: the shared code must have equivalent test coverage. Add tests if needed before proceeding.

#### 5c. Remove duplicate tests
- Delete the redundant test (keep the more thorough one).
- After each deletion: run `make test`.
- If coverage drops below threshold, `make test` exits non-zero — **revert immediately**. The "duplicate" test was covering something the other wasn't.

### Step 6 — Final verification

1. Run `make lint` — clippy with `-D warnings` and the format check must pass.
2. Run `make test` — tests must pass AND coverage must remain ≥ the baseline from Step 1.
3. Report: what was removed, what was merged, final coverage vs baseline.

(Only the 7 standard targets exist — `make lint` and `make test` cover formatting and coverage checks respectively.)

## Rules

- **No test coverage = do not touch.** If a file has no tests covering it, leave it alone entirely. You cannot safely dedup what you cannot verify.
- **Coverage must not drop.** If removing or merging code causes coverage to decrease, revert and investigate. The coverage floor from Step 1 is sacred.
- **One change at a time.** Make one dedup change, run tests, verify coverage. Never batch multiple dedup changes before testing.
- **When in doubt, leave it.** If two code blocks look similar but you're not 100% sure they're functionally identical, leave both. False dedup is worse than duplication.
- **Preserve public API surface.** Do not change `pub` function signatures, struct names, trait method names, or re-exports that downstream consumers (lql-lsp-rust, Basilisk, Deslop, SharpLsp) depend on. Internal refactoring only.
- **Three similar lines is fine.** Do not create abstractions for trivial duplication. The cure must not be worse than the disease. Only dedup when the shared logic is substantial (>10 lines) or when there are 3+ copies.
