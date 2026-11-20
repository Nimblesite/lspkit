#!/bin/sh
# [REL-CONSUMER-ASSETS] Check the release assets a consumer downloads instead
# of checking out the source (#42): build them for one runtime id, then build
# every fixture from the extracted consumer kit alone, outside this repository.
#
# Usage: scripts/consumer-kit-check.sh <dist-dir> <rid>
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <dist-dir> <rid>" >&2
  exit 2
fi
dist=$1
rid=$2
root=$(cd "$(dirname "$0")/.." && pwd)

sh "$root/scripts/release-assets.sh" ci "$dist" "$rid"
tar -tzf "$dist/lsp-profiler-dotnet-engine-ci-$rid.tar.gz" | grep -qx './lsp-profiler-dotnet-engine'

kit=$(tar -tzf "$dist/lspkit-consumer-kit-ci.tar.gz")
for want in dotnet/Directory.Build.props dotnet/fixtures/HotLeakApp/Program.cs \
  dotnet/fixtures/HotLeakAppFs/Program.fs docs/specs/LSP-PROFILER-SPEC.md \
  docs/specs/LSP-PULL-SPEC.md test-support/state.rs scripts/typediagram/run-cli.mjs; do
  if ! printf '%s\n' "$kit" | grep -qx "lspkit-consumer-kit-ci/$want"; then
    echo "consumer kit lacks $want" >&2
    exit 1
  fi
done
if printf '%s\n' "$kit" | grep -q '/bin/\|/obj/'; then
  echo "consumer kit ships build output" >&2
  exit 1
fi

# A consumer has only the kit: build each fixture from a fresh extraction.
outside=$(mktemp -d)
trap 'rm -rf "$outside"' EXIT
tar -xzf "$dist/lspkit-consumer-kit-ci.tar.gz" -C "$outside"
for fixture in HotLeakApp HotLeakAppFs; do
  dotnet build "$outside/lspkit-consumer-kit-ci/dotnet/fixtures/$fixture" \
    --configuration Release --nologo
done
echo "consumer kit: engine, files and standalone fixture builds OK"
