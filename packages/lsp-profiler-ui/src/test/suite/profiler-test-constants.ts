// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Shared constants and type helpers for profiler test suites.
 *
 * Extracted to avoid duplication across profiler.test.ts,
 * profiler-decorations.test.ts, and profiler-memory-integration.test.ts.
 *
 * Manifest contributions live in ./extension-manifest — they are read by every
 * suite, not just the profiler ones.
 */

/** Profiler client-side commands (registered in profiler.ts). */
export const PROFILER_CLIENT_COMMANDS = [
    'lspui.profiler.profileStart',
    'lspui.profiler.profileStop',
    'lspui.profiler.profileSnapshot',
    'lspui.profiler.profileAttachToDebug',
    'lspui.profiler.profileShowResults',
] as const;

/** Memory profiler client-side commands (registered in memory-profiler.ts). */
export const MEMORY_CLIENT_COMMANDS = [
    'lspui.profiler.memoryStart',
    'lspui.profiler.memorySnapshot',
    'lspui.profiler.memoryStop',
    'lspui.profiler.memoryReferences',
] as const;

/** Profiler server-side commands (advertised by LSP). */
export const PROFILER_SERVER_COMMANDS = [
    'lspui.lspProfiler.cpu.start',
    'lspui.lspProfiler.cpu.stop',
    'lspui.lspProfiler.cpu.snapshot',
    'lspui.lspProfiler.cpu.list',
    'lspui.lspProfiler.cpu.cooperativeScript',
    'lspui.lspProfiler.cpu.cooperativeAttach',
] as const;

/** Profiler configuration keys. */
export const PROFILER_SETTINGS = [
    'lspui.profiler.sampleRate',
    'lspui.profiler.includeNative',
    'lspui.profiler.lineThreshold',
    'lspui.profiler.functionThreshold',
    'lspui.profiler.maxDiagnosticsPerFile',
    'lspui.profiler.showInlineHeatMap',
] as const;

/** Additional profiler configuration keys. */
export const PROFILER_EXTRA_SETTINGS = [
    'lspui.profiler.profileOnLaunch',
    'lspui.profiler.preset',
] as const;
