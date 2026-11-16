#!/bin/sh
# Build the actual instrumented producer before exercising native CLI sessions.
# [LSP-PULL-TRANSPORT] [LSP-PULL-SALSA]
set -eu

coverage_environment="$(cargo llvm-cov show-env --sh)"
eval "$coverage_environment"
cargo llvm-cov clean --workspace
# Cargo's artifact metadata identifies the exact instrumented producer even when
# test executables live in a separate build directory. Never reuse a caller's override.
producer_manifest="$(mktemp)"
trap 'rm -f "$producer_manifest"' EXIT
cargo build -p lspkit-server --example diagnostic-reference --features pull --locked \
    --message-format=json-render-diagnostics > "$producer_manifest"
LSPKIT_REFERENCE_BIN="$(jq -ser '
    [.[] | select(.reason == "compiler-artifact" and .target.name == "diagnostic-reference")
        | .executable | select(. != null)]
    | if length == 1 then .[0] else error("expected one real reference producer") end
' "$producer_manifest")"
export LSPKIT_REFERENCE_BIN
cargo nextest run --workspace --all-targets --all-features --locked
cargo llvm-cov report --lcov --output-path lcov.info
