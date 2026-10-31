---
name: ci-prep
description: Prepares the current branch for CI by running the exact same steps locally and fixing issues. If CI is already failing, fetches the GH Actions logs first to diagnose. Use before pushing, when CI is red, or when the user says "fix ci".
argument-hint: "[--failing] [optional job name to focus on]"
---
<!-- agent-pmo:74cf183 -->

# CI Prep

Run what CI runs, locally, and fix it until green.

## If `--failing`: read the logs first

- Find the PR: `gh pr list --head "$(git branch --show-current)" --state open --json number --limit 1`. None → stop and say so.
- `gh pr checks <n>`, then `gh run view <run-id> --log-failed` for the latest run.
- Read every failed line. Note file, line and error. A job-name argument sets priority, but report everything.

## Read the workflow

- Read `.github/workflows/ci.yml` end to end (plus `release.yml` for release work). Extract the commands it actually runs, in order. Never assume.
- Note env vars, matrices and conditionals. Flag any target beyond the standard `make` set.
- In `release.yml`, flag as blockers: checking out `main` instead of the tag SHA, any git commit/push/tag mutation, version-bump commits after tagging, `sed` version stamping.

## Run and fix

- Run each CI command as CI does, in order. On failure: fix, re-run, then move on.
- Priority: `make fmt` → build → `make lint` → `make test`.
- Skip infra-only steps (checkout, toolchain setup, caching, uploads).

## Never

- Edit tests to pass, delete or skip tests, or remove assertions.
- Suppress lints (`#[allow]`, `oxlint-disable`, `#pragma warning disable`).
- Add `unsafe`, `unwrap`, `expect`, `panic!` or `todo!`.
- Lower `coverage-thresholds.json` or weaken any gate.
- Commit or push.

## Report

- Each step run and its result: pass, fail or fixed.
- Anything unfixed after 5 attempts, with the error and why.
- Whether the branch is ready to push. With `--failing`, list the exact commands that now pass; if the user pushes, watch the new run and loop back on failure.
