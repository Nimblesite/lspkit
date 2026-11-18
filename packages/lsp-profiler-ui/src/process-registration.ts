// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { viewId } from "./ids";
import { ProcessesProvider } from "./process-explorer";
import { registerLaunchCommands } from "./process-launch";
import { registerViewCommands } from "./process-commands";
import { bindPanel } from "./process-bind-panel";
import type { ProfilerStore as Store } from "./store";

/**
 * Register the Processes panel. The disposables own everything it created:
 * the commands, the tree view and the provider with its poll and effects, so
 * disposing the UI releases the panel. [PROF-UI-LIFETIME]
 */
export function registerProcessesView(store: Store): { provider: ProcessesProvider; disposables: vscode.Disposable[] } {
  const provider = new ProcessesProvider(store);
  const treeView = vscode.window.createTreeView(viewId(), { treeDataProvider: provider });
  bindPanel(store, treeView, provider);
  const commands = [...registerLaunchCommands(store, treeView), ...registerViewCommands(store, provider)];
  return { provider, disposables: [...commands, treeView, provider] };
}
