#!/usr/bin/env bash
# One-time npm setup. Publishes each package that does not exist yet (a Trusted
# Publisher can only be attached to an existing package), then attaches
# release.yml in the `release` environment as its Trusted Publisher, so every
# later version ships over OIDC. Idempotent: a published version and an
# existing trust are both skipped.
# [REL-OIDC] [REL-ORDER]
#
#   scripts/bootstrap-npm.sh v0.0.1
#
# Needs `npm login` with npm >= 11.10 (for `npm trust`).
set -euo pipefail

# shellcheck source=scripts/bootstrap-common.sh
source "$(dirname "$0")/bootstrap-common.sh"

mkdir -p "$work/dist"
npm run build --prefix "$work/packages/lsp-profiler-ui"
npm pack "$work/packages/lsp-profiler-ui" --pack-destination "$work/dist"
for tarball in "$work"/dist/*.tgz; do
    bash "$work/scripts/npm-publish-or-skip.sh" "$tarball"
done

readonly PACKAGE=@nimblesite/lsp-profiler-ui
trusts="$(npm trust list "$PACKAGE" --json)"
if [ "$(python3 -c 'import json, sys; print(len(json.loads(sys.argv[1])))' "$trusts")" -gt 0 ]; then
    echo "==> $PACKAGE already has a Trusted Publisher"
else
    npm trust github "$PACKAGE" --file release.yml --repo Nimblesite/lspkit --env release --allow-publish --yes
fi
