// Public surface of @nimblesite/lsp-profiler-ui. Implements [PROF-UI].
//
// A host extension creates a store from its `ProfilerHost` and registers the
// whole profiler UI in one call: processes view, CPU profiler, memory
// profiler and autopilot. Everything renders from the store's signals.

import { registerDebugProfilerSetup } from "./debug-link-setup";
import type * as vscode from "vscode";
import type { ProfilerHost } from "./host";
import { disposeMemoryProfiler, registerMemoryProfiler } from "./memory-profiler";
import { registerMemoryAutopilot } from "./memory-autopilot";
import { registerDebuggeePauseTracking } from "./debuggee-pause";
import type { ProcessesProvider } from "./process-explorer";
import { registerProcessesView } from "./process-registration";
import { disposeProfiler, registerProfiler } from "./profiler";
import type { ProfilerStore } from "./store";
import { createProfilerStore } from "./store";

export type { ProfilerHost, LaunchMetric } from "./host";
export type { ProfilerStore } from "./store";
export type { LogSink } from "./logger";
export { LogLevel, nullSink } from "./logger";
export { cmd, ctx, viewId, lsp, brand, configSection, processUriScheme } from "./ids";
export type { ProcessInfo, ProcessTreeItem } from "./process-explorer-rows";
export type { ProcessesProvider } from "./process-explorer";
export type { ProfileResult } from "./profiler-decorations";
export { PROFILER_SETTING_KEYS } from "./settings";

/** The registered profiler UI: its store and a way to tear it down. */
export interface ProfilerUi {
  readonly store: ProfilerStore;
  /** The processes tree's data provider: the rows the panel renders, for host suites to observe. */
  readonly processes: ProcessesProvider;
  dispose: () => void;
}

/** Register the complete profiler UI for `host`. */
export function registerProfilerUi(context: vscode.ExtensionContext, host: ProfilerHost): ProfilerUi {
  const store = createProfilerStore(host);
  const view = registerProcessesView(context, store);
  const disposables: vscode.Disposable[] = [
    ...registerDebuggeePauseTracking(),
    ...registerDebugProfilerSetup(store, host.debugType),
    ...registerProfiler(store),
    ...registerMemoryProfiler(store),
    ...registerMemoryAutopilot(store),
    ...view.disposables,
  ];
  context.subscriptions.push(...disposables);
  return { store, processes: view.provider, dispose: () => { disposeProfilerUi(store, disposables); } };
}

/** Tear down everything `registerProfilerUi` registered, and the store's state. */
function disposeProfilerUi(store: ProfilerStore, disposables: readonly vscode.Disposable[]): void {
  for (const disposable of disposables) {
    disposable.dispose();
  }
  disposeProfiler();
  disposeMemoryProfiler();
  store.reset();
}

// Observation seams for host test suites: what the UI last rendered.
export { profilerStatusText } from "./profiler-status";
export { memoryStatusText } from "./memory-status";
export { appliedProfileDecorations } from "./profiler-decorations";
export { appliedMemoryDecorations } from "./memory-decorations";
export { recordedOperations } from "./progress-ops";
export { panelMessage, panelBadge, processesViewState } from "./process-reactivity";
export { recordedAutopilotCaptures, recordedLeakOffers } from "./memory-autopilot";
export { flamegraphPanelOpen, flamegraphPanelState, profileHasNoUsableData, buildFlamegraphHtml } from "./profiler-flamegraph-html";
export { memoryDashboardState, buildMemoryDashboardHtml } from "./memory-dashboard";
export type { MemoryDashboardSnapshot } from "./memory-dashboard";
export { refGraphState, buildRefGraphHtml } from "./memory-ref-graph";
export { lastProfileResult } from "./profiler";
export { contextKeyValues } from "./context-keys";
export { lastMemorySnapshot } from "./memory-capture";
export { isDebuggeePaused } from "./debuggee-pause";
export { panelMessage as processesPanelMessage } from "./process-reactivity";
export type * as Wire from "./wire-generated";

export { type ProfilerPalette, resolvePalette, DEFAULT_PALETTE } from "./palette";
export { openFlamegraphWebview } from "./profiler-flamegraph-html";
export type { SampledView } from "./sampled-view";
