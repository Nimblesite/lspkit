#!/usr/bin/env bash
# Shared by bootstrap-crates.sh and bootstrap-npm.sh: a clean, stamped export of
# HEAD in "$work" (never the working tree), removed on exit. [REL-OIDC]
set -euo pipefail

tag="${1:?usage: $0 <vX.Y.Z>}"
repo="$(git rev-parse --show-toplevel)"
work="$(mktemp -d)"
trap 'rm -rf "${work:?}"' EXIT

git -C "$repo" archive HEAD | tar -x -C "$work"
for crate in "$work"/crates/*/; do cp "$work/LICENSE-MIT" "${crate}LICENSE-MIT"; done
for package in "$work"/packages/*/; do cp "$work/LICENSE-MIT" "${package}LICENSE"; done
(cd "$work" && npm ci --prefix packages/lsp-profiler-ui)
shipwright-version-stamp --tag "$tag" --root "$work"
