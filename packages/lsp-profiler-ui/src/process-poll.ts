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
import { setContextKey } from "./context-keys";
import { configSection, lspTable } from "./ids";
import { effect } from "@preact/signals-core";
import { State } from "vscode-languageclient/node";
import type { LanguageClient } from "vscode-languageclient/node";
import { Logger } from "./logger";
import { errorText } from "./failure";
import { executeServerCommand } from "./server-command";
import type { ProfilerStore as Store } from "./store";
import { toUiProcess } from "./wire-adapters";
import type { ProcessList } from "./wire-generated";

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

/** Mirror the language client's state to `<prefix>.profiler.serverState` so the welcome copy is honest. */
export function bindServerStateContextKey(store: Store): vscode.Disposable {
  let subscription: vscode.Disposable | undefined = undefined;
  const disposeEffect = effect(() => {
    const client = store.client.value;
    subscription?.dispose();
    subscription = client?.onDidChangeState(() => { publishClientState(client); });
    publishClientState(client);
  });
  return { dispose(): void { disposeEffect(); subscription?.dispose(); } };
}

/** Mirror one client's connection state to the `<prefix>.profiler.serverState` context key. */
function publishClientState(client: LanguageClient | undefined): void {
  setContextKey("serverState", clientStateLabel(client));
}

/** The welcome copy's label for a language client's connection state. */
function clientStateLabel(client: LanguageClient | undefined): string {
  if (client === undefined) { return "starting"; }
  if (client.state === State.Running) { return "running"; }
  if (client.state === State.Stopped) { return "stopped"; }
  return "starting";
}

/**
 * Mirror the store's fetch state to the `<prefix>.profiler.processesState` context key
 * that gates the welcome's loading/error/empty copy (#147). Reactive: the
 * effect re-fires whenever the store's process state changes, including the
 * initial "loading" seed.
 */
export function bindProcessesContextKey(store: Store): vscode.Disposable {
  const dispose = effect(() => {
    setContextKey("processesState", store.processes.value.fetch);
  });
  return { dispose };
}
