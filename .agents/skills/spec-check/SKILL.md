---
name: spec-check
description: Audit spec/plan documents against the codebase. Ensures every spec section has implementing code, tests, and matching logic. Use when the user says "check specs", "spec audit", or "verify specs".
argument-hint: "[optional spec ID or filename filter]"
---
<!-- agent-pmo:74cf183 -->

# Spec Check

Every spec section needs implementing code, tests, and logic that matches. Be pedantic and stop at the first failure.

`$ARGUMENTS` is optional: an exact ID (`PROF-WIRE-GEN`) checks that one section, and a path fragment (`lspkit`) checks those files. Empty means everything. No matches → error listing the spec files found.

## Find specs

- Search `docs/**/*.md`, `SPEC.md`, `PLAN.md` and `specs/**/*.md` for IDs matching `\[([A-Z][A-Z0-9]*(-[A-Z0-9]+)+)\]`.
- For each ID, capture the heading text and the section body (up to the next heading of the same or higher level).

## Validate IDs first

- **Invalid:** numbered (`[SPEC-001]`, `[CI-004]`), trailing numbers (`[FEAT-AUTH-01]`), or no group (`[TIMEOUT]`).
- **Valid:** hierarchical slugs, `[GROUP-TOPIC]` or `[GROUP-TOPIC-DETAIL]`, unique across the repo.
- **Clustered:** sections sharing a first word (the group) must be adjacent.
- **Complete:** headings that define behavior need an ID.
- Any violation → list them all with file:line and a suggested rename, then **stop**.

## Check each section, stopping at the first failure

- **Code refs:** grep the literal ID outside `docs/`, `target/`, `.git/` and `*.md`. Any comment style counts. None → violation.
- **Test refs:** grep the literal ID in test code (`tests/`, `*_test.rs`, `mod tests`, `packages/*/test/`, `src/test/`, `dotnet/*.Tests/`). None → violation.
- **Logic match:** read the section, then the code it cites. Flag wrong order, missing conditions, wrong comparisons, missing steps, wrong defaults, and extra behavior that contradicts the spec.

## Report

- **On failure:** only the first violation, quoting the spec text with file:line, quoting the code with file:line, stating the deviation and the exact fix. End with `spec-check FAILED. Fix the violation above and re-run.` Don't fix anything.
- **On success:** `spec-check PASSED.` plus a table of ID | Title | Code refs | Test refs | Logic, and a count of sections and files.
