#!/usr/bin/env bash
# Idempotent single-crate publish to crates.io. Treats an "already uploaded" /
# "already exists" response as success so re-running release.yml after a mid-batch
# failure is safe and never wedges on an already-published crate. [SWR-REL-CRATES]
set -uo pipefail

crate="${1:?usage: $0 <crate-name>}"

log="$(mktemp)"
trap 'rm -f "$log"' EXIT

# Stream to the CI log AND capture for the idempotency check. The working tree is
# intentionally dirty at release time (stamped versions + staged LICENSE-MIT),
# hence --allow-dirty; --locked keeps the published lockfile frozen.
# [SWR-VERSION-BUILD-STAMPING] [SWR-SEC-FROZEN-INSTALL]
echo "==> Publishing ${crate}"
cargo publish --locked --allow-dirty -p "${crate}" 2>&1 | tee "$log"
code="${PIPESTATUS[0]}"

if [ "$code" -ne 0 ] && grep -qE 'is already uploaded|already exists' "$log"; then
    printf '::notice::%s is already on crates.io at this version — skipping\n' "$crate"
    exit 0
fi

exit "$code"
