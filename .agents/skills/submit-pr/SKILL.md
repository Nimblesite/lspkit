---
name: submit-pr
description: Creates a pull request with a well-structured description after verifying CI passes. Use when the user asks to submit, create, or open a pull request.
disable-model-invocation: false
---
<!-- agent-pmo:74cf183 -->

# Submit PR

- **Gate:** `make ci` must pass. Skip if it already passed this session.
- **Diff:** `git diff main...HEAD` into the scratchpad. It's the only source of truth. Read big diffs in chunks.
- **Write from the diff only.** Ignore commit messages and branch names.
- **Body:** fill `.github/pull_request_template.md`:
  - TLDR (one sentence)
  - Added
  - Changed/Deleted
  - How tests prove it (name the tests)
  - Spec/doc changes
  - Breaking changes
- **Create:** `gh pr create`, linking the issue if one exists. Return the URL.

## Loop until green

The job isn't done when the PR opens. It's done when every check is green.

- **Watch:** `gh pr checks <n> --watch` until every check finishes.
- **Green:** report the URL and stop.
- **Red:**
  - Read every failed line: `gh run view <run-id> --log-failed`.
  - Fix the code, following [ci-prep](../ci-prep/SKILL.md).
  - Re-run the failing step locally until it passes.
  - Commit and push, then go back to **Watch**.
- **Stuck:** after 5 rounds on the same failure, report the error and what you tried.

## Rules

- No PR if `make ci` fails.
- Specific and tight. No vague placeholders.
- Never weaken a gate, skip a test or suppress a lint to turn a check green.
