// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import type { ProcessesProvider } from "./process-explorer";
import type { TreeItem } from "./process-explorer-rows";
import { bindProcessPolling, bindProcessesContextKey, bindServerStateContextKey } from "./process-poll";
import { bindDebuggeeTracking, bindProcessPanelReactivity } from "./process-reactivity";
import { subscribeRevision } from "./reactive-refresh";
import { ProcessDecorationProvider } from "./process-decoration";
import type { ProfilerStore as Store } from "./store";

/**
 * The panel is a pure projection of centralised state (#148): the store-side
 * poll feeds `store.processes` while the view is visible, the revision
 * subscription repaints on every store change, and the context-key mirror
 * keeps the welcome's loading/error/empty copy honest (#147).
 */
export function bindPanel(store: Store, treeView: vscode.TreeView<TreeItem>, provider: ProcessesProvider): void {
  provider.disposables.push(bindProcessPolling(store, treeView), bindProcessesContextKey(store), bindServerStateContextKey(store));
  subscribeRevision(store.processesRevision, provider);
  // React to the store's profiling state: live chrome, button-gating context
  // keys, and the active-row repaint ([PROF-UI-PROCESSES]).
  provider.disposables.push(
    // React to the store's profiling state: live chrome, button-gating context
    // keys, and the active-row repaint ([PROF-UI-PROCESSES]).
    bindProcessPanelReactivity(store, treeView, provider),
    // Reveal the inline Track Memory action only on the active debuggee row
    // ([PROF-UI-PROCESSES]) — memory tracking can't target external processes.
    bindDebuggeeTracking(store),
  );
  // Colour workspace rows green and non-debuggable rows grey ([PROF-UI-PROCESSES]).
  const decorations = new ProcessDecorationProvider(provider);
  provider.disposables.push(decorations, vscode.window.registerFileDecorationProvider(decorations));
}
