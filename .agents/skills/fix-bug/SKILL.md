---
name: fix-bug
description: Fix a bug using test-driven development. Use when the user reports a bug, describes unexpected behavior, wants to fix a defect, or says something is broken. Enforces a strict test-first workflow where a failing test must be written and verified before any fix is attempted.
argument-hint: "[bug description]"
allowed-tools: Read, Grep, Glob, Edit, Write, Bash
---
<!-- agent-pmo:74cf183 -->

# Fix Bug: test first

No fix until a test fails because of the bug.

- **Understand:** read the report ($ARGUMENTS) and the code, find the root cause, and state it in one line.
- **Write a failing test:** an E2E test that asserts the correct behavior, named after the bug, in the project's existing framework.
- **Prove it fails:** run only that test. It must fail *because of the bug*, not because of a typo or a bad import. If it passes or fails for the wrong reason, rewrite the test.
- **Show it:** report the test and the failure output, then keep going. Don't wait for confirmation.
- **Fix minimally:** the smallest change that fixes the bug. No refactors. Don't touch the test.
- **Prove it passes:** re-run the test, adjusting the fix until it's green.
- **Run everything:** `make test`. Fix regressions without weakening the new test.

## Rules

- Never edit the test to make it pass.
- One bug, one test, one fix.
- Can't write a test for it? Report why. Don't ship an untested fix.
