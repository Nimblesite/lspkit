---
name: release-review
description: Final gate before an lspkit release. A static, adversarial comparison of a clean main commit against the last release that hunts regressions, leaks, API breaks and weakened gates, and ends in a SAFE or BLOCK verdict. Use when the user says "release review", "regression review", "review against the release", or before tagging a release.
---

# Release Review

This is the last step before `v*` is tagged. **SAFE** means the commit can ship as is. You own every verdict.

## Preconditions (any failure → BLOCK, stop)

- `git fetch --tags origin`. The current branch is `main`, and `HEAD` equals `origin/main`.
- `git status --porcelain` is empty: no staged, unstaged or untracked files, and no stashes kept for this commit.
- The PR that produced `HEAD` passed every check: `gh pr list --state merged --search <sha>`, then `gh pr checks`.
- Baseline: `T=$(gh release view --json tagName -q .tagName)`. Review `git diff $T..HEAD`.

## Static comparison

Read every changed file in full, the old version beside the new (`git show $T:<path>`). Start with the highest blast radius:

1. 🛑 Memory lifetimes ([MEM-LIFETIME](../../../docs/specs/lspkit-spec.md#mem-lifetime-memory-lifetimes)):
   - a new ownership cycle;
   - a registration with no RAII owner;
   - an unowned `tokio::spawn`;
   - a keyed map with no eviction path;
   - a fix or long-lived structure with no leak regression.
2. Dispatch, cancellation and JSON-RPC framing (`crates/lspkit-server`), sidecar and correlator (`crates/lspkit`).
3. Live scheduler, watcher, VFS and position encoding.
4. Profiler service, sessions and sampler (`crates/lsp-profiler*`), the .NET engine (`dotnet/`), and the profiler UI webviews (`packages/lsp-profiler-ui`).
5. Release plumbing: `release.yml` and `scripts/release-assets.sh`.

Every removed guard, error path, bound or default is a suspect. Trace each change to its callers.

## Must-check list (each hit is a finding)

- **Public API:** list removed or changed `pub` items and exported TS symbols. Any break means the next version needs a minor bump, and the break must be listed in the release notes.
- **Gates weakened:**
  - in any of `coverage-thresholds.json`, `.github/workflows/`, `Makefile`, `deny.toml`, `rust-toolchain.toml`, lint configs (`[lints]`, `.oxlintrc.json`, `Directory.Build.props`): a lowered threshold, a raised timeout, a relaxed `if:`, or a lint turned off;
  - anywhere in source: a new `#[allow]`, `oxlint-disable` or `#pragma warning disable`.
- **Tests weakened:**
  - `git diff $T..HEAD --diff-filter=D --stat`;
  - removed `assert`/`expect` lines;
  - fewer permutations;
  - new `sleep` or timing probes;
  - tests that re-implement the code they test.
- **Dependencies:** check the `Cargo.lock` and npm lockfile diffs. A new crate or package needs a reason and a clean history, and versions must stay in sync across manifests and workflows.
- **Forbidden in production code:** `unwrap`/`expect`/`panic!`/`unsafe`, unbounded channels or queues, and `[GROUP-…]` citations that resolve to nothing.

## Legwork

- You may use at most two `pi` sessions (`zai/glm-5.3`). Give each one a single file set, the diff range and concrete questions. Register each on TMC.
- Their output is leads. Confirm every claim in code yourself. Kill a pi that stalls.
- ❌ Subagents, ❌ worktrees, ❌ edits or commits during the review.

## Tests

- This is static: ❌ `make test`/`make ci`, ❌ full suites, ❌ triggering Actions.
- You may run one targeted test (`cargo test -p <crate> --test <file> <name>`) only to confirm a specific suspect.

## Verdict

- A finding is **confirmed** only when you have traced the mechanism and can state input → wrong output. Drop anything you can't confirm.
- **BLOCK** on any confirmed regression, new leak, weakened gate or test, or unlisted API break. Otherwise **SAFE**, with the version bump the API diff calls for.
- File a GitHub issue **only** for a bug that is already in the released tag `$T`. Confirm it at `git show $T:<path>`, search `gh issue list --search` for a duplicate, then run `gh issue create --label bug` with the tag, file:line, repro, and expected vs actual.
- A bug new since `$T` gets **no issue**. It blocks the release: fix it through [fix-bug](../fix-bug/SKILL.md) on a branch and a PR, then re-run this review on the new `main`.
- This review never tags or releases.

## Reply

Give the verdict first, then confirmed findings ordered by severity, then exactly what you reviewed and what you did not.
