// The extension that hosts the library under test. Tests [PROF-UI-HOST].
//
// It is a minimal real host: a `ProfilerHost` with the `lspui` prefix, a
// language client talking to the mock server (`mock-server.ts`) over IPC,
// and the library registered exactly as a product extension registers it.
// Its exports are the seams the suites observe and steer.

import { signal } from "@preact/signals-core";
import * as path from "node:path";
import * as vscode from "vscode";
import { LanguageClient, TransportKind } from "vscode-languageclient/node";
import { registerProfilerUi, type LaunchMetric, type ProfilerHost, type ProfilerStore, type ProfilerUi } from "../../index";
import type { LogSink } from "../../logger";

/** What the suites get from `extensions.getExtension(...).exports`. */
export interface TestHostApi {
  readonly store: ProfilerStore;
  readonly host: ProfilerHost;
  readonly client: LanguageClient;
  readonly ui: ProfilerUi;
  /** Record a debuggee PID the way a host learns it from the DAP `process` event. */
  setDebuggeePid(sessionId: string, pid: number): void;
  clearDebuggeePid(sessionId: string): void;
  /** Lines the library logged, oldest first. */
  readonly logLines: readonly string[];
}

const logLines: string[] = [];

function line(level: string): (message: string) => void {
  return (message) => {
    logLines.push(`${level}: ${message}`);
    console.log(`[lspui] ${level}: ${message}`);
  };
}

const consoleSink: LogSink = { trace: line("trace"), debug: line("debug"), info: line("info"), warn: line("warn"), error: line("error") };

export async function activate(context: vscode.ExtensionContext): Promise<TestHostApi> {
  const serverModule = path.join(__dirname, "mock-server.js");
  const client = new LanguageClient(
    "lspui",
    "LSP Profiler UI test host",
    { module: serverModule, transport: TransportKind.ipc },
    { documentSelector: [{ language: "plaintext" }] },
  );
  const clientSignal = signal<LanguageClient | undefined>(undefined);
  const sessionIdToPid = signal<ReadonlyMap<string, number>>(new Map());
  const host: ProfilerHost = {
    idPrefix: "lspui",
    brand: "LspUi",
    configSection: "lspui",
    debugType: "lspui-debug",
    languageIds: ["plaintext"],
    client: clientSignal,
    logger: consoleSink,
    sessionIdToPid,
    launchConfig: (metric: LaunchMetric, program: string) => ({
      type: "lspui-debug",
      request: "launch",
      name: metric === "cpu" ? "Run & Profile CPU (Current File)" : "Run & Track Memory (Current File)",
      program,
      ...(metric === "cpu" ? { profileOnLaunch: true } : { stopOnEntry: true, memoryTrackOnLaunch: true }),
    }),
  };
  context.subscriptions.push(vscode.commands.registerCommand("lspui.restartServer", () => undefined));
  const ui = registerProfilerUi(context, host);
  context.subscriptions.push({ dispose: () => ui.dispose() });
  await client.start();
  context.subscriptions.push({ dispose: () => void client.stop() });
  clientSignal.value = client;
  return {
    store: ui.store,
    host,
    client,
    ui,
    setDebuggeePid(sessionId, pid) {
      sessionIdToPid.value = new Map([...sessionIdToPid.value, [sessionId, pid]]);
    },
    clearDebuggeePid(sessionId) {
      const next = new Map(sessionIdToPid.value);
      next.delete(sessionId);
      sessionIdToPid.value = next;
    },
    logLines,
  };
}

export function deactivate(): void {
  logLines.length = 0;
}
