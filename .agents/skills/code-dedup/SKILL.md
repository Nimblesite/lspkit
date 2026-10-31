---
name: code-dedup
description: Searches for duplicate code, duplicate tests, and dead code, then safely merges or removes them. Use when the user says "deduplicate", "find duplicates", "remove dead code", "DRY up", or "code dedup". Requires test coverage — refuses to touch untested code.
---
<!-- agent-pmo:74cf183 -->

# Code Dedup

Merge duplicates and delete dead code, only where tests prove it's safe.

## Gate

- `make test` must be green (it's fail-fast and enforces `coverage-thresholds.json`). Red → stop.
- Record the coverage figure. That's the floor.
- Confirm workspace lints still deny `unsafe_code`, `missing_docs`, `unwrap_used`, `expect_used`, `panic`, `todo`.
- Untested files are off limits.

## Scan (list findings, change nothing yet)

- **Duplicates:** Deslop `top-offenders`, then `find-similar` per hit (fused ≥ 0.85 is a candidate). Check across crates and packages; shared engine logic belongs behind `EngineApi` in `lspkit`.
- **Dead code:** `make lint` surfaces private dead code. For `pub` items, search every call site (tests, doc-tests, build scripts, configs). Dead only at zero references.
- **Duplicate tests:** the same assertions on the same path, or fixtures copied between crates. Keep the coarser E2E test.

## Apply, one change at a time

- Change → `make test` → coverage ≥ floor? Keep it. Otherwise revert immediately.
- Merged code goes to the crate that owns the domain. Update every call site.
- When deleting a duplicate test, keep the more thorough one.

## Rules

- Not sure two blocks behave identically? Leave both. A false merge is worse than duplication.
- Don't change public API (`pub` signatures, names, re-exports). SharpLsp, Deslop, Basilisk and lql-lsp-rust depend on it.
- Finish with `make lint` and `make test`. Report what was removed and merged, and coverage against the floor.
