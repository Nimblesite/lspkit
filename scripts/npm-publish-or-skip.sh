#!/usr/bin/env bash
# Idempotent single-package publish to npm. Publishes the exact tarball the
# release checksummed, signed and attested — never a fresh pack — and skips a
# version that is already on the registry, so re-running release.yml after a
# mid-batch failure is safe. [REL-ORDER] [SWR-REL-NPM]
#
# Authentication is npm OIDC Trusted Publishing (npm >= 11.5.1 exchanges the
# GitHub OIDC token for a short-lived publish token): no NPM_TOKEN exists.
# release.yml sets NPM_CONFIG_PROVENANCE=true so each package links to its
# workflow run; the one-time bootstrap runs off-CI, where npm has no
# provenance to sign. [SWR-SEC-OIDC-PUBLISH] [SWR-SEC-PROVENANCE]
set -euo pipefail

tarball="${1:?usage: $0 <package.tgz>}"

# Read name + version from the tarball's own manifest with a JSON parser.
manifest="$(tar -xOzf "$tarball" package/package.json)"
spec="$(node -e 'const p = JSON.parse(process.argv[1]); process.stdout.write(p.name + "@" + p.version);' "$manifest")"

# `npm view` prints nothing for an unpublished version of an existing package
# and fails for an unpublished package; both mean "publish".
if [ -n "$(npm view "$spec" version 2>/dev/null)" ]; then
    printf '::notice::%s is already on npm — skipping\n' "$spec"
    exit 0
fi

echo "==> Publishing ${spec}"
npm publish "$tarball" --access public
