#!/bin/sh
# [REL-CONSUMER-ASSETS] Package what consumers would otherwise need a source
# checkout for (#42), as GitHub Release assets beside the crates and npm package:
#
#   lsp-profiler-dotnet-engine-<version>-<rid>.tar.gz
#       The .NET engine `lsp-profiler-dotnet` launches, one per runtime id.
#       Framework-dependent (net8.0, RollForward=LatestMajor). Point
#       LSP_PROFILER_DOTNET_ENGINE at the extracted executable, or place it
#       beside the language server.
#   lspkit-consumer-kit-<version>.tar.gz
#       The profiler fixtures (dotnet/fixtures) with the dotnet/Directory.Build.props
#       they build with, so they build from the kit alone; the specs consumers cite
#       (LSP-PROFILER-SPEC.md, LSP-PULL-SPEC.md), test-support/ and
#       scripts/typediagram/run-cli.mjs.
#
# Usage: scripts/release-assets.sh <version> <dist-dir> [rid ...]
set -eu

if [ "$#" -lt 2 ]; then
  echo "usage: $0 <version> <dist-dir> [rid ...]" >&2
  exit 2
fi
version=$1
dist=$2
shift 2
if [ "$#" -eq 0 ]; then
  set -- linux-x64 linux-arm64 osx-x64 osx-arm64 win-x64 win-arm64
fi

root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$dist"
dist=$(cd "$dist" && pwd)

for rid in "$@"; do
  out="$work/engine-$rid"
  dotnet publish "$root/dotnet/LspProfiler.DotNet.Engine" \
    --configuration Release --runtime "$rid" --self-contained false \
    --output "$out" --nologo
  case "$rid" in
    win-*) executable=lsp-profiler-dotnet-engine.exe ;;
    *) executable=lsp-profiler-dotnet-engine ;;
  esac
  if [ ! -f "$out/$executable" ]; then
    echo "release-assets: $rid publish has no $executable" >&2
    exit 1
  fi
  cp "$root/LICENSE-MIT" "$out/LICENSE"
  tar -czf "$dist/lsp-profiler-dotnet-engine-$version-$rid.tar.gz" -C "$out" .
done

kit="$work/lspkit-consumer-kit-$version"
mkdir -p "$kit/dotnet" "$kit/docs/specs" "$kit/scripts/typediagram"
# Fixtures ship as source; their build output never does.
tar -cf - -C "$root/dotnet" --exclude=bin --exclude=obj fixtures | tar -xf - -C "$kit/dotnet"
cp "$root/dotnet/Directory.Build.props" "$kit/dotnet/"
cp "$root/docs/specs/LSP-PROFILER-SPEC.md" "$root/docs/specs/LSP-PULL-SPEC.md" "$kit/docs/specs/"
cp -R "$root/test-support" "$kit/test-support"
cp "$root/scripts/typediagram/run-cli.mjs" "$kit/scripts/typediagram/"
cp "$root/LICENSE-MIT" "$kit/LICENSE"
tar -czf "$dist/lspkit-consumer-kit-$version.tar.gz" -C "$work" "lspkit-consumer-kit-$version"

ls -l "$dist"
