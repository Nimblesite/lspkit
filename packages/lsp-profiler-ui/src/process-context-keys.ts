// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
import type * as vscode from "vscode";
import { setContextKey } from "./context-keys";
import { effect } from "@preact/signals-core";
import { State } from "vscode-languageclient/node";
import type { LanguageClient } from "vscode-languageclient/node";
import type { ProfilerStore as Store } from "./store";

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
