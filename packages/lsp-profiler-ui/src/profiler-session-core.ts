// Implements [PROF-UI-STATE], [PROF-CPU-COOPERATIVE]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Shared CPU-session machinery: the LSP command table, sampler settings,
 * session adoption, and the attach-to-debuggee request both the panel and the
 * launch flows reuse.
 */

import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";
import { brand, configSection, lspTable } from "./ids";
import { Logger } from "./logger";
import type { ProfilerStore as Store } from "./store";
import { reportFailure } from "./failure";
import { executeServerCommand } from "./server-command";
import { toStartedSession } from "./wire-adapters";
import type { StartedSession } from "./wire-adapters";
import type { StartProfileResult } from "./wire-generated";
import { startWhenRuntimeReady } from "./profiler-launch";

/** LSP command names ([PROF-WIRE-METHODS]). */
export const CPU_CMD = lspTable({
  start: "cpu.start",
  stop: "cpu.stop",
  snapshot: "cpu.snapshot",
  list: "cpu.list",
  cooperativeScript: "cpu.cooperativeScript",
  cooperativeAttach: "cpu.cooperativeAttach",
});

/** Default sample rate fallback (Hz). */
const DEFAULT_SAMPLE_RATE = 100;

/** The configured sampler: its rate (Hz) and whether native frames are kept. */
export function samplerSettings(): { sample_rate: number; include_native: boolean } {
  const cfg = vscode.workspace.getConfiguration(configSection());
  return {
    sample_rate: cfg.get<number>("profiler.sampleRate", DEFAULT_SAMPLE_RATE),
    include_native: cfg.get<boolean>("profiler.includeNative", false),
  };
}

/**
 * Adopt a freshly started session into the shared reactive state
 * ([PROF-UI-PROCESSES]). A start the server answered with nothing adopts nothing.
 */
export function adoptSession(store: Store, raw: StartProfileResult | undefined, announce: (session: StartedSession) => string): void {
  if (raw === undefined) { return; }
  const result = toStartedSession(raw);
  store.profilerActive(result.pid, result.sessionId);
  Logger.info(
    `Profiling started: PID ${String(result.pid)}, ${result.runtime} ${result.runtimeVersion}, session ${result.sessionId}`,
  );
  vscode.window.showInformationMessage(announce(result));
}

/** A start that never reached "active" clears its spinner. */
export function settleStart(store: Store): void {
  if (store.profiler.value.cpu !== "active") { store.profilerStopped(); }
}

/** The single progress title every CPU-start flow shares. */
export function cpuStartTitle(): string {
  return `${brand()}: Starting CPU profiler`;
}

/** The debuggee a CPU start targets, plus an optional "still launching" retry gate. */
export interface AttachTarget {
  readonly pid: number;
  readonly launching?: () => boolean;
}

/**
 * Start a CPU session on the debuggee `pid` and adopt it; a just-launched one
 * is retried while `launching()` holds. The caller owns the CPU leg.
 */
export async function attachToDebuggee(store: Store, client: LanguageClient, target: AttachTarget): Promise<void> {
  const { pid, launching = (): boolean => false } = target;
  const args = { pid, ...samplerSettings() };
  const send = async (): Promise<StartProfileResult | undefined> => {
    const result = await executeServerCommand<StartProfileResult>(client, CPU_CMD.start, args);
    return result;
  };
  try {
    const raw = await startWhenRuntimeReady(send, launching);
    adoptSession(store, raw, (result) => `${brand()}: Profiling debug session (PID ${String(result.pid)})`);
  } catch (error: unknown) {
    reportFailure("Profile attach-to-debug", error);
  }
}
