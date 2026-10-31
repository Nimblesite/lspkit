// Shared helpers of the UI suites: reach the test host, wait for its mock
// server, and poll for reactive outcomes. Tests [PROF-TESTING].
import type { ReadonlySignal, Signal } from "@preact/signals-core";
import { signal } from "@preact/signals-core";
import type { LaunchMetric, ProcessInfo, ProfilerHost, ProfilerStore } from "../../index";
import { nullSink } from "../../index";
import type { ProcessInfo as WireProcess } from "../../wire-generated";
import * as fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";
import { closeAllEditors, pollUntilResult, removeTestDir } from "../../testing/webview-contract-helpers";
import type { TestHostApi } from "../host/extension";

export { closeAllEditors, pollUntilResult, removeTestDir } from "../../testing/webview-contract-helpers";
export type { PollOptions } from "../../testing/webview-contract-helpers";
export { POLL_INTERVAL_MS, WAIT_MS } from "../../timeouts";

/** The host extension's identifier (`publisher.name` of src/test/host/package.json). */
export const EXTENSION_ID = "lspui.lsp-profiler-ui-test-host";
export const SUITE_SETUP_TIMEOUT_MS = 90_000;
export const SERVER_START_WAIT_MS = 60_000;
export const COMMAND_WAIT_MS = 1000;
export const DEFAULT_POLL_INTERVAL_MS = 100;

/**
 * Yield one microtask.
 *
 * A stub standing in for an async API call must be `async` (the type it stands
 * in for demands a promise) yet has nothing real to wait for. Yielding once
 * keeps it behaving like the call it replaces instead of resolving eagerly.
 */
export async function nextMicrotask(): Promise<void> {
  await Promise.resolve();
}

/** Write to a signal a consumer only reads (test seams). */
export function seedSignal<TValue>(target: ReadonlySignal<TValue>, value: TValue): void {
  (target as Signal<TValue>).value = value;
}

/**
 * True for an object that stands in for a language client.
 *
 * Test doubles fabricate the handful of members a suite exercises, so the only
 * honest check available is that the value is an object; the predicate names
 * that licensing point instead of hiding it in a double assertion.
 */
export function isClientStub(value: unknown): value is LanguageClient {
  return typeof value === "object" && value !== null;
}

/** A language client stand-in exposing only `members`. */
export function fakeLanguageClient(members: Record<string, unknown>): LanguageClient {
  if (!isClientStub(members)) {
    throw new Error("a language client stand-in must be an object");
  }
  return members;
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

/** Wait until the mock language server reports itself running. */
export async function waitForLspReady(): Promise<void> {
  const api = await hostApi();
  await pollUntilResult({
    fn: () => api.store.client.value?.isRunning() === true,
    until: (running) => running,
    timeoutMs: SERVER_START_WAIT_MS,
    intervalMs: DEFAULT_POLL_INTERVAL_MS,
    description: "the mock language server start",
  });
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

/** A temp workspace dir plus a hot source file the mock server's reports point at. */
export async function setupLspTestSuite(tmpDirPrefix: string): Promise<{ tmpDir: string; hotFile: string }> {
  const tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), tmpDirPrefix));
  const hotFile = path.join(tmpDir, "hot.txt");
  await fs.writeFile(hotFile, "def main():\n    pass\ndef hot_function():\n    total = 0\n    return total\n");
  await waitForLspReady();
  await configureMock({ hotFile, processes: null, memoryMode: "Direct" });
  await closeAllEditors();
  return { tmpDir, hotFile };
}

export async function teardownLspTestSuite(tmpDir: string): Promise<void> {
  await removeTestDir(tmpDir);
}

/** A detached host for store-level tests: no client, no debuggee, `lspui` identity. */
export function fakeHost(overrides: Partial<ProfilerHost> = {}): ProfilerHost {
  return {
    idPrefix: "lspui",
    brand: "LspUi",
    configSection: "lspui",
    debugType: "lspui-debug",
    languageIds: ["plaintext"],
    languageNames: ["Plain Text"],
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
  await fs.writeFile(filePath, content, "utf8");
  const uri = vscode.Uri.file(filePath);
  const doc = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(doc, { preview: false });
  return { doc, uri };
}
