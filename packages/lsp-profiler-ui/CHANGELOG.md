# Changelog

## Unreleased

- `ProfilerUi.dispose()` now disposes the processes tree view and its provider, including the provider's poll and effects.
- DAP output, stop-state and pending resumes for a debug session are dropped when the session terminates.
- The memory timeline and the autopilot capture ledger keep their newest 120 entries.
- `FileLogSink` gains `dispose(): Promise<void>` and caps its log: past an optional byte limit (8 MiB by default) it continues in `<path>.1`, so at most two capped files stay on disk.
- The profiler's .NET engine, fixtures, specs and test-support ship as GitHub Release assets (`lsp-profiler-dotnet-engine-<version>-<rid>.tar.gz`, `lspkit-consumer-kit-<version>.tar.gz`), so consumers no longer need a source checkout.

## 0.4.0

### Breaking

These functions now return a `Promise` and must be awaited:

- `buildFlamegraphHtml(result)` now returns `Promise<string>`.
- `applyProfileDecorations(result)`, `applyMemoryDecorations(result)` and `applyLeakDecorations(result)` now return `Promise<void>`.
- `removeTestDir(dir)` (from `testing`) now returns `Promise<void>`.

A caller that does not await them gets a pending `Promise` instead of the HTML string, or runs ahead before the decorations are applied or the directory is removed.
