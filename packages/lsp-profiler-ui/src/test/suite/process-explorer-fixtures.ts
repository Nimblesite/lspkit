// Shared fixtures for the Processes panel suites ([PROF-UI-PROCESSES]): a
// stubbed process table, a store over a fake client that serves it, and
// readers for what a row carries.

import * as vscode from "vscode";
import type { ProcessInfo } from "../../process-explorer";
import { ProcessesProvider } from "../../process-explorer";
import { createProfilerStore } from "../../store";
import type { ProfilerStore as Store } from "../../store";
import { fakeHost, fakeLanguageClient, seedSignal, toWireProcess, ungroup } from "./test-helpers";
import { numberField, rawField, recordArrayField } from "../../unknown-shape";

export const MB = 1024 * 1024;

/** CPython's advertised profilers: sampled CPU, and tracemalloc injected through the debugger (Courier). */
export const CPYTHON_PROFILERS = {
  cpu: { live_progress: true, line_level: true, native_frames: false, cooperative: true },
  memory: { mode: { kind: "Courier" }, allocation_sites: true, references: false, objects_by_type: false, gc_collect: true },
} as const;

/** A representative process table covering launchers, users, and versions. */
export const STUB_PROCESSES: readonly ProcessInfo[] = [
  {
    pid: 100, ppid: 1, name: "python3.12", interpreterPath: "/usr/bin/python3.12",
    script: "/app/web.py", runtime: "python", runtimeVersion: "3.12.1", cpuPercent: 5, memoryBytes: 50 * MB,
    runtimeSecs: 10, user: "alice", requiresElevation: false,
    inWorkspace: true, launcher: null, debuggable: true, undebuggableReason: null,
  },
  {
    pid: 200, ppid: 1, name: "python3.11", interpreterPath: "/usr/bin/python3.11",
    script: "/app/worker.py", runtime: "python", runtimeVersion: "3.11.7", cpuPercent: 42, memoryBytes: 10 * MB,
    runtimeSecs: 99, user: "bob", requiresElevation: true,
    inWorkspace: false, launcher: null, debuggable: true, undebuggableReason: null,
  },
  {
    pid: 300, ppid: 200, name: "python3.12", interpreterPath: "/usr/bin/python3.12",
    script: "/app/uvicorn.py", runtime: "python", runtimeVersion: "3.12.1", cpuPercent: 1, memoryBytes: 99 * MB,
    runtimeSecs: 5, user: "alice", requiresElevation: false,
    inWorkspace: false, launcher: "uvicorn", debuggable: true, undebuggableReason: null,
  },
];

/** One `workspace/executeCommand` request captured by the recording client. */
export interface RecordedRequest {
  readonly command: string;
  readonly arguments: readonly unknown[];
}

/**
 * Build a Store whose LSP client returns the given process table and records
 * every executeCommand request so tests can assert what was sent (e.g. that an
 * inline action really issued `basilisk.profiler.start` with the row's PID).
 */
export function storeWith(processes: readonly ProcessInfo[], requests?: RecordedRequest[]): Store {
  const store = createProfilerStore(fakeHost());
  // A stand-in for the members the provider actually calls; the rest of
  // `LanguageClient` is what the double leaves out on purpose.
  const client = fakeLanguageClient({
    isRunning: (): boolean => true,
    // The server names its runtime; the row must show that name, never a
    // guess derived from the wire id's spelling ([PROF-UI-CAPABILITIES]). It
    // advertises both profilers, as tracemalloc-backed CPython does: an absent
    // `memory` block would mean the runtime has no memory profiler at all.
    initializeResult: { capabilities: { experimental: { lspProfiler: { runtimes: [{ id: "python", display_name: "CPython", source_entries: true, ...CPYTHON_PROFILERS }] } } } },
    onDidChangeState: (): vscode.Disposable => ({ dispose: (): undefined => undefined }),
    // A real request crosses the wire; this double records what it was asked
    // for and then yields once, so an awaiting caller sees the same ordering.
    sendRequest: async (_method: string, param?: RecordedRequest): Promise<unknown> => {
      if (param !== undefined) { requests?.push(param); }
      if (param?.command === "lspui.lspProfiler.cpu.start") { return undefined; }
      const response = { processes: processes.map((process) => toWireProcess(process)) };
      await Promise.resolve();
      return response;
    },
  });
  seedSignal(store.client, client);
  return store;
}


/**
 * Build a flat provider over `store` and pull the process table into the store
 * through the real store-side fetch path — getChildren() is a pure projection
 * of centralised state and never fetches on its own (#148).
 */
export async function loadedProvider(store: Store): Promise<ProcessesProvider> {
  ungroup(store);
  const provider = new ProcessesProvider(store);
  await provider.refreshNow();
  return provider;
}

/** The providers the running test created; its `teardown` disposes them. */
const createdProviders: ProcessesProvider[] = [];

/** Remember `provider` so the running test's `teardown` disposes it. */
export function tracked(provider: ProcessesProvider): ProcessesProvider {
  createdProviders.push(provider);
  return provider;
}

/** Dispose every provider the test that just finished created. */
export function disposeProviders(): void {
  for (const provider of createdProviders.splice(0)) {
    provider.dispose();
  }
}

/**
 * A command runner for the row actions: it records what the UI ran and settles
 * on the next microtask, the way a real command host resolves its thenable.
 */
export function recordingRunner(executed: string[]): (command: string) => Thenable<unknown> {
  return async (command) => {
    executed.push(command);
  };
}

/** The `basilisk.profiler.start` requests among the recorded ones. */
export function profilerStarts(requests: readonly RecordedRequest[]): RecordedRequest[] {
  return requests.filter((req) => req.command === "lspui.lspProfiler.cpu.start");
}

/** The pid argument of a recorded `basilisk.profiler.start` request. */
export function startPid(request: RecordedRequest): unknown {
  return rawField(request.arguments[0], "pid");
}

// `process` and `members` are attached to the tree item by the provider under
// test and are not part of `vscode.TreeItem`. They are read back by name rather
// than asserted onto the item, so a provider that stops attaching one fails the
// assertion instead of handing the test an `undefined` it would compare away.

/** Read the PID a process row carries (the arg passed to inline commands). */
export function pidOf(item: vscode.TreeItem): number | undefined {
  return numberField(rawField(item, "process"), "pid");
}

/** The PIDs of the members a group row carries. */
export function memberPids(item: vscode.TreeItem): number[] {
  return recordArrayField(item, "members")
    .map((member) => numberField(member, "pid"))
    .filter((pid): pid is number => pid !== undefined);
}

/** Read a group header's label (a plain string at runtime). */
export function labelText(item: vscode.TreeItem): string {
  return typeof item.label === "string" ? item.label : "";
}

// ── Display-cue helpers ([PROF-UI-PROCESSES]) ──────────────────────

/** The vscode.ThemeColor / ThemeIcon `id` a row or decoration resolves to. */
export function colorId(value: { id?: string } | undefined): string | undefined {
  return value?.id;
}
export function iconId(item: vscode.TreeItem): string | undefined {
  return item.iconPath instanceof vscode.ThemeIcon ? item.iconPath.id : undefined;
}
export function resourceUri(item: vscode.TreeItem): vscode.Uri | undefined {
  return item.resourceUri;
}
/** The row's tooltip narrowed to its string form (rowTooltip always returns one). */
export function tooltipText(item: vscode.TreeItem): string {
  return typeof item.tooltip === "string" ? item.tooltip : "";
}

/** Drop the pinned "Run & …(Current File)" launch-action rows from a root listing. */
export function processRows(rows: vscode.TreeItem[]): vscode.TreeItem[] {
  return rows.filter((row) => row.contextValue !== "launchAction");
}
/** Just the pinned launch-action rows. */
export function actionRows(rows: vscode.TreeItem[]): vscode.TreeItem[] {
  return rows.filter((row) => row.contextValue === "launchAction");
}

/** A debugger-machinery row: listed, but non-debuggable (the 🚫 / grey / sunk case). */
export const MACHINERY: ProcessInfo = {
  pid: 900, ppid: 1, name: "python3.12", interpreterPath: "/usr/bin/python3.12",
  script: null, runtime: "python", runtimeVersion: "3.12.1", cpuPercent: 99, memoryBytes: 5 * MB,
  runtimeSecs: 3, user: "alice", requiresElevation: false,
  inWorkspace: false, launcher: null, debuggable: false,
  undebuggableReason: "debugger machinery",
};
