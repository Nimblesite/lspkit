#!/usr/bin/env bash
# One-time first publish of the packages that do not exist on a registry yet.
# Trusted Publishing (OIDC) can only be configured on an existing package, so
# each new name is published once from a maintainer machine; every later
# version ships from release.yml. Builds from a clean export of HEAD, never the
# working tree. Idempotent: already-published versions are skipped.
# [REL-OIDC] [REL-ORDER]
#
#   scripts/bootstrap-registries.sh v0.0.1
#
# Needs `cargo login` and `npm login` (npm >= 11.5.1).
set -euo pipefail

tag="${1:?usage: $0 <vX.Y.Z>}"
repo="$(git rev-parse --show-toplevel)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

export_head() {
    git -C "$repo" archive HEAD | tar -x -C "$work"
    for crate in "$work"/crates/*/; do cp "$work/LICENSE-MIT" "${crate}LICENSE-MIT"; done
    for package in "$work"/packages/*/; do cp "$work/LICENSE-MIT" "${package}LICENSE"; done
}

prepare() {
    (cd "$work" && npm ci --prefix packages/lsp-profiler-ui)
    shipwright-version-stamp --tag "$tag" --root "$work"
    (cd "$work" && cargo update --workspace && node scripts/typediagram/generate.mjs)
}

publish_crates() {
    for crate in lsp-profiler lsp-profiler-dotnet; do
        (cd "$work" && bash scripts/cargo-publish-or-skip.sh "$crate")
    done
}

publish_npm() {
    mkdir -p "$work/dist"
    npm run build --prefix "$work/packages/lsp-profiler-ui"
    npm pack "$work/packages/lsp-profiler-ui" --pack-destination "$work/dist"
    for tarball in "$work"/dist/*.tgz; do
        bash "$work/scripts/npm-publish-or-skip.sh" "$tarball"
    done
}

export_head
prepare
publish_crates
publish_npm
