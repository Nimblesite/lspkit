// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The store-side source of the Processes list (#148).
 *
 * The OS has no push event for "a process appeared", so the list is
 * genuinely poll-sourced — but per the architecture the poll belongs HERE,
 * feeding the store's `processes` Signal, never inside the panel: the tree
 * provider stays a pure projection of centralised state and owns no timer.
 * Polling runs only while the panel is visible, so navigating away stops the
 * LSP round-trips.
 */

import * as vscode from "vscode";
import { configSection, lspTable } from "./ids";
import { Logger } from "./logger";
import { errorText } from "./failure";
import { executeServerCommand } from "./server-command";
import type { ProfilerStore as Store } from "./store";
import { toUiProcess } from "./wire-adapters";
import type { ProcessList } from "./wire-generated";

export { bindProcessesContextKey, bindServerStateContextKey } from "./process-context-keys";

/** LSP command name ([PROF-WIRE-METHODS]). */
const LSP_CMD = lspTable({ processes: "processes" });

/** Default poll interval (ms) when the setting is absent. */
const DEFAULT_REFRESH_MS = 2000;

/**
 * Fetch the process table from the LSP into the store's `processes` Signal.
 * Every outcome lands honestly ([PROF-UI-PROCESSES], #147): no running
 * client stays "loading" (the serverState welcome owns the copy), a failure is
 * "error", and only a successful fetch may render "No processes running".
 */
export async function fetchProcessesIntoStore(store: Store): Promise<void> {
  const client = store.client.value;
  if (client?.isRunning() !== true) {
    store.processesLoading();
    return;
  }
  try {
    const roots = (vscode.workspace.workspaceFolders ?? []).map((folder) => folder.uri.fsPath);
    const result = await executeServerCommand<ProcessList>(client, LSP_CMD.processes, { workspace_roots: roots });
    store.processesLoaded((result?.processes ?? []).map((process) => toUiProcess(process)));
  } catch (error: unknown) {
    Logger.error(`Processes fetch failed: ${errorText(error)}`);
    store.processesFetchFailed();
  }
}

/**
 * Poll the LSP into the store while the panel is visible: an immediate fetch on
 * becoming visible (no blank 2-second first paint), then one per interval. The
 * returned disposable stops the timer and the visibility subscription.
 */
export function bindProcessPolling(
  store: Store,
  treeView: Pick<vscode.TreeView<unknown>, "visible" | "onDidChangeVisibility">,
): vscode.Disposable {
  const poll = processPoll(store);
  const subscription = treeView.onDidChangeVisibility((event) => {
    if (event.visible) { poll.start(); } else { poll.stop(); }
  });
  if (treeView.visible) { poll.start(); }
  return { dispose(): void { subscription.dispose(); poll.stop(); } };
}

/** A fetch now and then every configured interval, until stopped. */
function processPoll(store: Store): { start: () => void; stop: () => void } {
  let timer: ReturnType<typeof setInterval> | undefined = undefined;
  return {
    start(): void {
      if (timer !== undefined) { return; }
      const intervalMs = vscode.workspace.getConfiguration(configSection()).get<number>("profiler.processRefreshMs", DEFAULT_REFRESH_MS);
      void fetchProcessesIntoStore(store);
      timer = setInterval(() => { void fetchProcessesIntoStore(store); }, intervalMs);
    },
    stop(): void {
      if (timer !== undefined) { clearInterval(timer); timer = undefined; }
    },
  };
}


