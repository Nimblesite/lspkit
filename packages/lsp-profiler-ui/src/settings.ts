// The `profiler.*` settings the library reads under the host's section.
// A host's package.json contributes each one as `<section>.profiler.<key>`;
// its manifest test checks the list against this constant.

export const PROFILER_SETTING_KEYS = [
  "profiler.palette",
  "profiler.sampleRate",
  "profiler.includeNative",
  "profiler.lineThreshold",
  "profiler.functionThreshold",
  "profiler.maxDiagnosticsPerFile",
  "profiler.showInlineHeatMap",
  "profiler.profileOnLaunch",
  "profiler.processRefreshMs",
  "profiler.preset",
  "profiler.autoSnapshotOnPause",
  "profiler.autoSnapshot",
  "profiler.autoSnapshotInterval",
] as const;
