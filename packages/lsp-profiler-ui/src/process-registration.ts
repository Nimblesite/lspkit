// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { viewId } from "./ids";
import { ProcessesProvider } from "./process-explorer";
import { registerLaunchCommands } from "./process-launch";
import { registerViewCommands } from "./process-commands";
import { bindPanel } from "./process-bind-panel";
import type { ProfilerStore as Store } from "./store";

/**
 * Register the Processes panel. Returns command disposables for
 * `singletonDisposables`; the tree view and provider go to subscriptions.
 */
export function registerProcessesView(
  context: vscode.ExtensionContext,
  store: Store,
): { provider: ProcessesProvider; disposables: vscode.Disposable[] } {
  const provider = new ProcessesProvider(store);
  const treeView = vscode.window.createTreeView(viewId(), { treeDataProvider: provider });
  context.subscriptions.push(treeView, provider);
  bindPanel(store, treeView, provider);
  return { provider, disposables: [...registerLaunchCommands(store, treeView), ...registerViewCommands(store, provider)] };
}
