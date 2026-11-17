# Changelog

## Unreleased

- The profiler's .NET engine, fixtures, specs and test-support ship as GitHub Release assets (`lsp-profiler-dotnet-engine-<version>-<rid>.tar.gz`, `lspkit-consumer-kit-<version>.tar.gz`), so consumers no longer need a source checkout.

## 0.4.0

### Breaking

These functions now return a `Promise` and must be awaited:

- `buildFlamegraphHtml(result)` now returns `Promise<string>`.
- `applyProfileDecorations(result)`, `applyMemoryDecorations(result)` and `applyLeakDecorations(result)` now return `Promise<void>`.
- `removeTestDir(dir)` (from `testing`) now returns `Promise<void>`.

A caller that does not await them gets a pending `Promise` instead of the HTML string, or runs ahead before the decorations are applied or the directory is removed.
