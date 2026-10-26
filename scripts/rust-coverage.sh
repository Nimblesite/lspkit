#!/bin/sh
# Build the actual instrumented producer before exercising native CLI sessions.
# [LSP-PULL-TRANSPORT] [LSP-PULL-SALSA]
set -eu

coverage_environment="$(cargo llvm-cov show-env --sh)"
eval "$coverage_environment"
cargo llvm-cov clean --workspace
# CI must exercise this profile's producer, never a caller's stale override.
unset LSPKIT_REFERENCE_BIN
cargo build -p lspkit-server --example diagnostic-reference --features pull --locked
cargo nextest run --workspace --all-targets --all-features --locked
cargo llvm-cov report --lcov --output-path lcov.info
