---
name: upgrade-packages
description: Upgrade all Cargo dependencies to their latest versions. Use when the user says "upgrade packages", "update dependencies", "bump versions", "update packages", or "upgrade deps".
argument-hint: "[--check-only] [--major] [crate-name]"
---
<!-- agent-pmo:74cf183 -->

# Upgrade Packages

`--check-only` lists what's outdated and stops. `--major` allows breaking bumps. Any other argument names a single package.

## List first

- **Cargo:** `cargo outdated --workspace`, `cargo update --dry-run`. Versions live once, in the root `[workspace.dependencies]`.
- **npm:** `npm outdated` in each `packages/*`. TypeScript stays on `^7`.
- **.NET:** `dotnet list dotnet/ package --outdated`.
- With `--check-only`, report and stop here.

## Read the docs

- Fetch the [cargo update docs](https://doc.rust-lang.org/cargo/commands/cargo-update.html) before running anything. Don't guess flags.

## Upgrade

- **Cargo:** `cargo update`. With `--major`, `cargo update --breaking`. For one crate: `cargo update -p <crate> [--precise <v>]`.
- **npm:** `npm install <pkg>@latest` per package.
- **Versions:** keep them in sync across manifests, `.github/workflows/` and `.devcontainer/`.

## Verify

- `make ci`. On failure, read the changelog (tower-lsp, rmcp, tokio, lsp-types and tree-sitter break often) and fix the source.
- Stuck after 3 attempts on one failure: report the package and the error.

## Never

- Edit lockfiles by hand.
- Downgrade, unless rolling back a broken upgrade.
- Remove packages, unless they're deprecated and replaced.
- Suppress post-upgrade warnings.
- Commit.

## Report

- old → new for each upgraded package.
- Skipped packages, and why.
- The `make ci` result.
- Breaking changes fixed, with file references.
- Failures.
