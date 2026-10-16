// Shared helpers of the UI suites: reach the test host, wait for its mock
// server, and poll for reactive outcomes. Tests [PROF-TESTING].
import { signal, type ReadonlySignal, type Signal } from "@preact/signals-core";
import type { LaunchMetric, ProfilerHost } from "../../host";
import { nullSink } from "../../logger";
import type { ProcessInfo } from "../../process-explorer-rows";
import type { ProfilerStore } from "../../store";
import type { ProcessInfo as WireProcess } from "../../wire-generated";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";
import { delay } from "../../timeouts";
import type { TestHostApi } from "../host/extension";

export { POLL_INTERVAL_MS, WAIT_MS } from "../../timeouts";

/** The host extension's identifier (`publisher.name` of src/test/host/package.json). */
export const EXTENSION_ID = "lspui.lsp-profiler-ui-test-host";
export const SUITE_SETUP_TIMEOUT_MS = 90_000;
export const SERVER_START_WAIT_MS = 60_000;
export const COMMAND_WAIT_MS = 1_000;
export const DEFAULT_POLL_INTERVAL_MS = 100;

/** Write to a signal a consumer only reads (test seams). */
export function seedSignal<T>(signal: ReadonlySignal<T>, value: T): void {
  (signal as Signal<T>).value = value;
}

/** A language client stand-in exposing only `members`. */
export function fakeLanguageClient(members: Record<string, unknown>): LanguageClient {
  return members as unknown as LanguageClient;
}

/** The test host's API once it is active. */
export async function hostApi(): Promise<TestHostApi> {
  const extension = vscode.extensions.getExtension<TestHostApi>(EXTENSION_ID);
  if (extension === undefined) {
    throw new Error(`${EXTENSION_ID} is not installed in the test host`);
  }
  return extension.isActive ? extension.exports : extension.activate();
}

/** The live store, or `undefined` before activation. */
export function getStore(): TestHostApi["store"] | undefined {
  return vscode.extensions.getExtension<TestHostApi>(EXTENSION_ID)?.exports?.store;
}

/** The `workspace/executeCommand` names the server advertised at initialize. */
export function advertisedServerCommands(): ReadonlySet<string> {
  const client = getStore()?.client.value;
  const commands = client?.initializeResult?.capabilities.executeCommandProvider?.commands ?? [];
  return new Set(commands);
}

/** The wire form of a UI process, for test doubles that stand in for the server. */
export function toWireProcess(process: ProcessInfo): WireProcess {
  return {
    pid: process.pid,
    ppid: process.ppid,
    name: process.name,
    runtime: process.runtime,
    ...(process.runtimeVersion === null ? {} : { runtime_version: process.runtimeVersion }),
    ...(process.interpreterPath === null ? {} : { executable: process.interpreterPath }),
    ...(process.script === null ? {} : { entry: process.script }),
    cpu_percent: process.cpuPercent,
    memory_bytes: process.memoryBytes,
    runtime_secs: process.runtimeSecs,
    ...(process.user === null ? {} : { user: process.user }),
    requires_elevation: process.requiresElevation,
    in_workspace: process.inWorkspace,
    ...(process.launcher === null ? {} : { launcher: process.launcher }),
    attachable: process.debuggable,
    ...(process.undebuggableReason === null ? {} : { unattachable_reason: process.undebuggableReason }),
  };
}

/**
 * Turn the Processes panel's grouping off. It opens grouped by runtime version
 * ([PROF-UI-PROCESSES]); tests of row order, filtering and row shape read a flat list.
 */
export function ungroup(store: ProfilerStore): void {
  while (store.processes.value.groupMode !== "none") {
    store.cycleProcessGroup();
  }
}

/** Steer the mock server. */
export async function configureMock(options: Record<string, unknown>): Promise<void> {
  const api = await hostApi();
  await api.client.sendRequest("workspace/executeCommand", { command: "lspui.mock.configure", arguments: [options] });
}

export async function waitForLspReady(): Promise<void> {
  const api = await hostApi();
  const deadline = Date.now() + SERVER_START_WAIT_MS;
  while (Date.now() < deadline) {
    if (api.store.client.value?.isRunning() === true) {
      return;
    }
    await delay(DEFAULT_POLL_INTERVAL_MS);
  }
  throw new Error("the mock language server did not start");
}

export interface PollOptions<T> {
  readonly fn: () => Promise<T> | T;
  readonly until: (value: T) => boolean;
  readonly timeoutMs: number;
  readonly intervalMs?: number;
  readonly description?: string;
}

/** The launch configuration the test host builds for `metric`; asserts it exists. */
export function launchConfigOf(metric: LaunchMetric, program: string): vscode.DebugConfiguration {
  const config = testHostLaunchConfig(metric, program);
  if (config === undefined) {
    throw new Error(`the test host builds no ${metric} launch configuration`);
  }
  return config;
}

function testHostLaunchConfig(metric: LaunchMetric, program: string): vscode.DebugConfiguration | undefined {
  return {
    type: "lspui-debug",
    request: "launch",
    name: metric === "cpu" ? "Run & Profile CPU (Current File)" : "Run & Track Memory (Current File)",
    program,
    ...(metric === "cpu" ? { profileOnLaunch: true } : { stopOnEntry: true, memoryTrackOnLaunch: true }),
  };
}

/** Poll until `until` accepts the value; the two-argument form uses a 10s budget. */
export async function pollUntilResult<T>(options: PollOptions<T>): Promise<T>;
export async function pollUntilResult<T>(fn: () => Promise<T> | T, until: (value: T) => boolean): Promise<T>;
export async function pollUntilResult<T>(first: PollOptions<T> | (() => Promise<T> | T), until?: (value: T) => boolean): Promise<T> {
  const options: PollOptions<T> = typeof first === "function" ? { fn: first, until: until ?? (() => true), timeoutMs: 10_000 } : first;
  const deadline = Date.now() + options.timeoutMs;
  let last: T = await options.fn();
  while (!options.until(last)) {
    if (Date.now() >= deadline) {
      throw new Error(`${options.description ?? "condition"} not met within ${options.timeoutMs}ms (last: ${JSON.stringify(last)})`);
    }
    await delay(options.intervalMs ?? DEFAULT_POLL_INTERVAL_MS);
    last = await options.fn();
  }
  return last;
}

export async function closeAllEditors(): Promise<void> {
  await vscode.commands.executeCommand("workbench.action.closeAllEditors");
}

/** A temp workspace dir plus a hot source file the mock server's reports point at. */
export async function setupLspTestSuite(tmpDirPrefix: string): Promise<{ tmpDir: string; hotFile: string }> {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), tmpDirPrefix));
  const hotFile = path.join(tmpDir, "hot.txt");
  fs.writeFileSync(hotFile, "def main():\n    pass\ndef hot_function():\n    total = 0\n    return total\n");
  await waitForLspReady();
  await configureMock({ hotFile, processes: null, memoryMode: "Direct" });
  await closeAllEditors();
  return { tmpDir, hotFile };
}

export function removeTestDir(dir: string): void {
  if (dir === "" || !fs.existsSync(dir)) {
    return;
  }
  fs.rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
}

export function teardownLspTestSuite(tmpDir: string): void {
  removeTestDir(tmpDir);
}

/** A detached host for store-level tests: no client, no debuggee, `lspui` identity. */
export function fakeHost(overrides: Partial<ProfilerHost> = {}): ProfilerHost {
  return {
    idPrefix: "lspui",
    brand: "LspUi",
    configSection: "lspui",
    debugType: "lspui-debug",
    languageIds: ["plaintext"],
    client: signal<LanguageClient | undefined>(undefined),
    logger: nullSink,
    sessionIdToPid: signal<ReadonlyMap<string, number>>(new Map()),
    launchConfig: testHostLaunchConfig,
    ...overrides,
  };
}

/** Write `content` to `filename` under `tmpDir` and open it in an editor. */
export async function openSourceFile(
  tmpDir: string,
  filename: string,
  content: string,
): Promise<{ doc: vscode.TextDocument; uri: vscode.Uri }> {
  const filePath = path.join(tmpDir, filename);
  fs.writeFileSync(filePath, content, "utf8");
  const uri = vscode.Uri.file(filePath);
  const doc = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(doc, { preview: false });
  return { doc, uri };
}
