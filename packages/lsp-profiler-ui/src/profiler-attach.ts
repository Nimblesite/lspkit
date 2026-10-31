// Implements [PROF-UI-STATE], [PROF-CPU-COOPERATIVE]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Starting a CPU session: from the Processes panel, from a debug session and
 * on launch. Every start adopts its session into the store, whose signal drives
 * the status bar, the panel chrome and the stop/snapshot/progress routing.
 */

import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";
import { brand } from "./ids";
import type { ProfilerStore as Store } from "./store";
import { reportFailure } from "./failure";
import { withUserProgress } from "./progress-ops";
import { executeServerCommand } from "./server-command";
import type { StartProfileResult } from "./wire-generated";
import { CPU_CMD, adoptSession, attachToDebuggee, cpuStartTitle, samplerSettings, settleStart } from "./profiler-session-core";

// Re-exported so the profiler keeps one import site for every CPU start seam.
export { CPU_CMD } from "./profiler-session-core";
export { profileOnLaunch } from "./profile-on-launch";

/**
 * The running client, when a CPU start may begin. Only the CPU leg blocks a CPU
 * start: a CPU profile may begin while memory tracking is live, but never on
 * top of another CPU profile ([PROF-UI-PROCESSES]). Otherwise the reason is shown.
 */
function clientForStart(store: Store, busy: () => string): LanguageClient | undefined {
  const client = store.client.value;
  if (client?.isRunning() !== true) {
    vscode.window.showWarningMessage(`${brand()}: Language server not running.`);
    return undefined;
  }
  if (store.cpuBusy.value) {
    vscode.window.showWarningMessage(busy());
    return undefined;
  }
  return client;
}

/** A "stop the current CPU session first" warning naming the active CPU profile. */
function busyMessage(store: Store): string {
  const session = store.profiler.value;
  const pid = session.cpuPid === undefined ? "" : ` PID ${String(session.cpuPid)}`;
  return `${brand()}: Already profiling${pid}. Stop the current CPU session first.`;
}

/**
 * Start profiling a specific PID: the panel's one-click path. The LSP resolves
 * `preset` server-side; the sampler settings apply when the preset is
 * `"default"`. Implements [PROF-UI-PROCESSES].
 */
export async function startProfilingForPid(store: Store, pid: number, preset: string): Promise<void> {
  const client = clientForStart(store, () => busyMessage(store));
  if (client === undefined) { return; }
  store.profilerStarting();
  try {
    await withUserProgress(cpuStartTitle(), async (report) => {
      report(`Attaching to PID ${String(pid)}…`);
      const raw = await executeServerCommand<StartProfileResult>(client, CPU_CMD.start, { pid, preset, ...samplerSettings() });
      adoptSession(store, raw, (result) => `${brand()}: Profiling PID ${String(result.pid)} (${result.runtime} ${result.runtimeVersion})`);
    });
  } catch (error: unknown) {
    reportFailure("Profile start", error);
  } finally {
    settleStart(store);
  }
}

/**
 * Profile the active debug session's debuggee: the PID captured from the DAP
 * `process` event, so the sampler profiles the process the debugger is on.
 */
export async function handleProfileAttachToDebug(store: Store): Promise<void> {
  const session = vscode.debug.activeDebugSession;
  if (session === undefined) {
    vscode.window.showWarningMessage(`${brand()}: No active debug session to profile.`);
    return;
  }
  const client = clientForStart(store, () => `${brand()}: Already profiling (session ${store.profiler.value.cpuSessionId ?? "?"}).`);
  if (client === undefined) { return; }
  await attachToActiveDebuggee(store, client, session);
}

/** Resolve the active session's debuggee PID and start profiling it. */
async function attachToActiveDebuggee(store: Store, client: LanguageClient, session: vscode.DebugSession): Promise<void> {
  const pid = store.getDebuggeeProcessId(session.id);
  if (pid === undefined) {
    vscode.window.showWarningMessage(
      `${brand()}: The debuggee process isn't ready yet — let it start running, then run “Profile Debug Session” again.`,
    );
    return;
  }
  await attachToDebuggee(store, client, { pid });
}
