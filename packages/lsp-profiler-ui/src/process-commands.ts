// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { brand, cmd, lsp, viewId } from "./ids";
import type { ProcessesProvider } from "./process-explorer";
import type { ProcessTreeItem } from "./process-explorer-rows";
import type { ProfilerStore as Store } from "./store";
import { errorText } from "./failure";
import { executeServerCommand } from "./server-command";
import { fetchProcessesIntoStore } from "./process-poll";
import { withViewProgress } from "./progress-ops";

/** The view's toolbar commands and the per-row actions. */
export function registerViewCommands(store: Store, provider: ProcessesProvider): vscode.Disposable[] {
  return [
    // Manual refresh runs under the view's progress bar so the click visibly
    // does something; returns the promise so callers/tests can await it.
    vscode.commands.registerCommand(cmd("refreshProcesses"), async () => { await withViewProgress(viewId(), "Refresh processes", async () => { await provider.refreshNow(); }); }),
    vscode.commands.registerCommand(cmd("sortProcesses"), () => { provider.cycleSortMode(); }),
    vscode.commands.registerCommand(cmd("groupProcesses"), () => { provider.cycleGroupMode(); }),
    ...registerRowCommands(store, provider),
  ];
}

/** The filter and the per-row actions: kill, copy PID, reveal the entry script. */
function registerRowCommands(store: Store, provider: ProcessesProvider): vscode.Disposable[] {
  return [
    vscode.commands.registerCommand(cmd("filterProcesses"), async () => {
      const input = await vscode.window.showInputBox({ prompt: "Filter processes by name, script, or PID", placeHolder: "e.g. uvicorn" });
      provider.setFilter(input ?? "");
    }),
    vscode.commands.registerCommand(cmd("killProcess"), async (item?: ProcessTreeItem) => { await killProcess(store, item); }),
    vscode.commands.registerCommand(cmd("copyProcessPid"), (item?: ProcessTreeItem) => {
      if (item !== undefined) {
        void vscode.env.clipboard.writeText(String(item.process.pid));
        vscode.window.showInformationMessage(`${brand()}: Copied PID ${String(item.process.pid)}`);
      }
    }),
    vscode.commands.registerCommand(cmd("revealProcessScript"), (item?: ProcessTreeItem) => { revealProcessScript(store, item); }),
  ];
}

function revealProcessScript(store: Store, item?: ProcessTreeItem): void {
  const scriptPath = item?.process.script;
  if (typeof scriptPath === "string" && store.sourceEntries(item?.process.runtime ?? "")) {
    void vscode.window.showTextDocument(vscode.Uri.file(scriptPath));
  } else {
    vscode.window.showInformationMessage(`${brand()}: This process has no entry file to reveal.`);
  }
}

type RunningClient = NonNullable<Store["client"]["value"]>;

/**
 * Kill a row's process after a modal confirmation; the server refuses PIDs it
 * does not recognise as profilable, so this can never hit the editor itself.
 */
async function killProcess(store: Store, item?: ProcessTreeItem): Promise<void> {
  const client = store.client.value;
  if (item === undefined || client?.isRunning() !== true) {
    return;
  }
  if (!await confirmKill(item)) {
    return;
  }
  await performKill(store, client, item);
}

/** Ask the user to confirm terminating the row's process. */
async function confirmKill(item: ProcessTreeItem): Promise<boolean> {
  const question = `${brand()}: Kill process ${item.process.name} (PID ${String(item.process.pid)})? It will be terminated immediately.`;
  const choice = await vscode.window.showWarningMessage(question, { modal: true }, "Kill");
  return choice === "Kill";
}

/** Send the kill command and refresh the panel, surfacing any server refusal. */
async function performKill(store: Store, client: RunningClient, item: ProcessTreeItem): Promise<void> {
  try {
    await executeServerCommand<unknown>(client, lsp("kill"), { pid: item.process.pid });
    vscode.window.showInformationMessage(`${brand()}: Killed PID ${String(item.process.pid)}.`);
    await fetchProcessesIntoStore(store);
  } catch (error: unknown) {
    vscode.window.showErrorMessage(`${brand()}: ${errorText(error)}`);
  }
}
