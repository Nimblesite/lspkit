#!/usr/bin/env bash
# One-time first publish of the npm packages that do not exist yet; a Trusted
# Publisher can only be attached to an existing package, and every later
# version ships from release.yml. Idempotent: a published version is skipped.
# [REL-OIDC] [REL-ORDER]
#
#   scripts/bootstrap-npm.sh v0.0.1
#
# Needs `npm login` (npm >= 11.5.1).
set -euo pipefail

# shellcheck source=scripts/bootstrap-common.sh
source "$(dirname "$0")/bootstrap-common.sh"

mkdir -p "$work/dist"
npm run build --prefix "$work/packages/lsp-profiler-ui"
npm pack "$work/packages/lsp-profiler-ui" --pack-destination "$work/dist"
for tarball in "$work"/dist/*.tgz; do
    bash "$work/scripts/npm-publish-or-skip.sh" "$tarball"
done
