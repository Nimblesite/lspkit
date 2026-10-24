---
name: upgrade-packages
description: Upgrade all Cargo dependencies to their latest versions. Use when the user says "upgrade packages", "update dependencies", "bump versions", "update packages", or "upgrade deps".
argument-hint: "[--check-only] [--major] [crate-name]"
---
<!-- agent-pmo:74cf183 -->

# Upgrade Packages

Upgrade Cargo dependencies to their latest compatible (or latest major, if `--major`) versions.

## Arguments

- `--check-only` — List outdated crates without upgrading. Stop after Step 2.
- `--major` — Include major version bumps (breaking changes). Without this flag, stay within semver-compatible ranges.
- Any other argument is treated as a specific crate name to upgrade (instead of all crates).

## Step 1 — Detect manifests

This is a Rust workspace. Manifests to consider:

| Manifest file | Purpose |
|---|---|
| `Cargo.toml` (workspace root) | `[workspace.dependencies]` table — single source of truth for versions |
| `crates/*/Cargo.toml` | Per-crate dependency tables (should re-export from `[workspace.dependencies]`) |

If `Cargo.toml` is missing, stop and tell the user.

## Step 2 — List outdated crates

Run before upgrading anything. Show the user what will change.

```bash
cargo outdated --workspace        # install: cargo install cargo-outdated
cargo update --dry-run
```

**Read the docs:** https://doc.rust-lang.org/cargo/commands/cargo-update.html

If `--check-only` was passed, **stop here** and report the outdated list.

## Step 3 — Read the official upgrade docs

**Before running any upgrade command, you MUST fetch and read the official documentation URL above.** Use WebFetch. Do not guess at flags from memory. Pay particular attention to `cargo update --breaking` (cargo 1.84+) — that is the flag for cross-major upgrades.

## Step 4 — Upgrade crates

```bash
cargo update                          # semver-compatible updates
# --major flag:
cargo update --breaking               # major version bumps (cargo 1.84+)
```

For a specific crate: `cargo update -p <crate>` (or `cargo update -p <crate> --precise <version>` for pinning).

Workspace dependency versions are centralised in `[workspace.dependencies]` at the root `Cargo.toml`. For breaking version bumps that require manifest edits (e.g. `tokio 1 -> 2`), update the version string in `[workspace.dependencies]` once — per-crate `Cargo.toml` files that use `tokio = { workspace = true }` will pick it up automatically.

## Step 5 — Verify the upgrade

After upgrading, run the full CI pipeline locally:

```bash
make ci
```

If `make ci` fails:
1. Read the failure output carefully.
2. Check the changelog / release notes for the upgraded crates. For tower-lsp, rmcp, tokio, lsp-types, and tree-sitter, breaking changes are common across majors — fetch the relevant release notes URL.
3. Fix breaking changes in source code (NOT in tests, NOT via `#[allow]` suppressions).
4. Re-run `make ci`.
5. If stuck after 3 attempts on the same failure, report it to the user with the error details and the crate that caused it.

## Step 6 — Report

Provide a summary:

- Crates upgraded (old version -> new version)
- Crates skipped (and why, e.g., major version bump without `--major` flag)
- `make ci` result after upgrade
- Any breaking changes that were fixed (with file references)
- Any crates that could not be upgraded (with error details)

## Rules

- **Always list outdated crates first** before upgrading anything.
- **Always read the official `cargo update` docs** before running upgrade commands.
- **Always run `make ci` after upgrading** to catch breakage immediately.
- **Never remove crates** unless they were explicitly deprecated and replaced (e.g., a future migration from `tower-lsp` to `async-lsp`).
- **Never downgrade crates** unless rolling back a broken upgrade.
- **Never modify `Cargo.lock` manually** — let cargo regenerate it.
- **Never add `#[allow(...)]`** to silence post-upgrade warnings — fix the underlying code.
- **Commit nothing** — leave changes in the working tree for the user to review.

## Success criteria

- All outdated crates upgraded to latest compatible (or latest major if `--major`).
- `make ci` passes (lint + test + build, with coverage threshold).
- User has a clear summary of what changed.
