#!/usr/bin/env bash
# One-time crates.io setup. Publishes each crate that does not exist yet (a
# Trusted Publisher can only be attached to an existing crate), then attaches
# release.yml in the `release` environment as the Trusted Publisher of every
# published crate, so every later version ships over OIDC. Idempotent: a
# published version and an existing config are both skipped.
# [REL-OIDC] [REL-ORDER]
#
#   scripts/bootstrap-crates.sh v0.0.1
#
# Needs `cargo login` with a token scoped publish-new + trusted-publishing.
set -euo pipefail

# shellcheck source=scripts/bootstrap-common.sh
source "$(dirname "$0")/bootstrap-common.sh"

readonly NEW_CRATES=(lsp-profiler lsp-profiler-dotnet)
readonly PUBLISHED_CRATES=(lsp-profiler lsp-profiler-dotnet lspkit lspkit-server)
readonly API=https://crates.io/api/v1/trusted_publishing/github_configs
readonly AGENT="lspkit-bootstrap (https://github.com/Nimblesite/lspkit)"

(cd "$work" && cargo update --workspace && node scripts/typediagram/generate.mjs)
for crate in "${NEW_CRATES[@]}"; do
    (cd "$work" && bash scripts/cargo-publish-or-skip.sh "$crate")
done

token="$(python3 -c 'import pathlib, tomllib; print(tomllib.loads((pathlib.Path.home() / ".cargo/credentials.toml").read_text())["registry"]["token"])')"

api() { curl -sS --fail-with-body -A "$AGENT" -H "Authorization: $token" "$@"; }

for crate in "${PUBLISHED_CRATES[@]}"; do
    total="$(api "$API?crate=$crate" | python3 -c 'import json, sys; print(json.load(sys.stdin)["meta"]["total"])')"
    if [ "$total" -gt 0 ]; then echo "==> $crate already has a Trusted Publisher"; continue; fi
    body="$(python3 -c 'import json, sys; print(json.dumps({"github_config": {"crate": sys.argv[1], "repository_owner": "Nimblesite", "repository_name": "lspkit", "workflow_filename": "release.yml", "environment": "release"}}))' "$crate")"
    api -X POST -H "Content-Type: application/json" -d "$body" "$API" >/dev/null
    echo "==> $crate: Trusted Publisher Nimblesite/lspkit release.yml (environment release)"
done
