// Implements [PROF-UI-STATE], [PROF-CPU-COOPERATIVE]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Starting a CPU session: from the Processes panel, from a debug session and
 * on launch. Every start adopts its session into the store, whose signal drives
 * the status bar, the panel chrome and the stop/snapshot/progress routing.
 */

import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";
import { brand, configSection, lspTable } from "./ids";
import { Logger } from "./logger";
import type { ProfilerStore as Store } from "./store";
import { evaluateInDebugSession, waitForStoppedFrame } from "./dap-evaluate";
import { reportFailure } from "./failure";
import { withUserProgress } from "./progress-ops";
import { shouldProfileOnLaunch, startWhenRuntimeReady, waitForDebuggeePid } from "./profiler-launch";
import { executeServerCommand } from "./server-command";
import { toStartedSession } from "./wire-adapters";
import type { StartedSession } from "./wire-adapters";
import type { CooperativeScript, StartProfileResult } from "./wire-generated";

/** LSP command names ([PROF-WIRE-METHODS]). */
export const CPU_CMD = lspTable({
  start: "cpu.start",
  stop: "cpu.stop",
  snapshot: "cpu.snapshot",
  list: "cpu.list",
  cooperativeScript: "cpu.cooperativeScript",
  cooperativeAttach: "cpu.cooperativeAttach",
});

/** Ack printed by the injected cooperative sampler ([PROF-CPU-COOPERATIVE]). */
const COOPERATIVE_ACK = "__LSPPROF_CPU_ACK__";

/** Default sample rate fallback (Hz). */
const DEFAULT_SAMPLE_RATE = 100;

/** The configured sampler: its rate (Hz) and whether native frames are kept. */
function samplerSettings(): { sample_rate: number; include_native: boolean } {
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
function adoptSession(store: Store, raw: StartProfileResult | undefined, announce: (session: StartedSession) => string): void {
  if (raw === undefined) { return; }
  const result = toStartedSession(raw);
  store.profilerActive(result.pid, result.sessionId);
  Logger.info(
    `Profiling started: PID ${String(result.pid)}, ${result.runtime} ${result.runtimeVersion}, session ${result.sessionId}`,
  );
  vscode.window.showInformationMessage(announce(result));
}

/** A start that never reached "active" clears its spinner. */
function settleStart(store: Store): void {
  if (store.profiler.value.cpu !== "active") { store.profilerStopped(); }
}

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

/** The single progress title every CPU-start flow shares. */
function cpuStartTitle(): string {
  return `${brand()}: Starting CPU profiler`;
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

/** The debuggee a CPU start targets, plus an optional "still launching" retry gate. */
interface AttachTarget {
  readonly pid: number;
  readonly launching?: () => boolean;
}

/**
 * Start a CPU session on the debuggee `pid` and adopt it; a just-launched one
 * is retried while `launching()` holds. The caller owns the CPU leg.
 */
async function attachToDebuggee(store: Store, client: LanguageClient, target: AttachTarget): Promise<void> {
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

/** Shown once per session: a profiling launch neutralises breakpoints (ux-6). */
let breakpointSuppressionNoticeShown = false;

/**
 * Tell the user, once, that a profiling launch runs to completion with their
 * breakpoints disabled, or a Run & Profile silently never stops at a breakpoint
 * the gutter still shows armed (ux-6). Only fires when breakpoints are set.
 */
function notifyBreakpointsSuppressedForProfiling(): void {
  if (breakpointSuppressionNoticeShown || vscode.debug.breakpoints.length === 0) {
    return;
  }
  breakpointSuppressionNoticeShown = true;
  void vscode.window.showInformationMessage(
    `${brand()}: Profiling run — your breakpoints are disabled so the program runs to completion. Launch without profiling to debug with breakpoints.`,
  );
}

/** "Profile on Launch": profile a debug session that asked for it while the CPU leg is idle. */
export function profileOnLaunch(store: Store, session: vscode.DebugSession): void {
  if (!shouldProfileOnLaunch(session) || store.profiler.value.cpu !== "idle") { return; }
  Logger.info(`Profile on Launch: auto-profiling debug session ${session.id}`);
  notifyBreakpointsSuppressedForProfiling();
  void startProfilerOnLaunch(store, session.id);
}

/**
 * Auto-start for a freshly launched debug session under one progress
 * notification with stage messages; the status bar spins until the live sample
 * counter takes over, so a click is never followed by silence ([PROF-UI-STATE]).
 */
async function startProfilerOnLaunch(store: Store, debugSessionId: string): Promise<void> {
  store.profilerStarting();
  try {
    await withUserProgress(cpuStartTitle(), async (report) => {
      // On macOS, task ports are gated behind entitled debuggers, so a runtime that can
      // only be sampled from outside is sampled by its cooperative in-process
      // sampler, injected at the entry pause ([PROF-CPU-COOPERATIVE]). A runtime
      // that samples itself, such as .NET over EventPipe, attaches as anywhere.
      if (process.platform === "darwin" && cpuIsCooperativeEverywhere(store)) {
        await startCooperativeProfileOnLaunch(store, report);
        return;
      }
      await attachOnLaunch(store, debugSessionId, report);
    });
  } finally {
    settleStart(store);
  }
}

/** Wait for the debuggee's PID, which arrives with the DAP `process` event, then attach to it. */
async function attachOnLaunch(store: Store, debugSessionId: string, report: (message: string) => void): Promise<void> {
  report("Waiting for the program to start…");
  const ready = await waitForDebuggeePid(store, debugSessionId);
  const pid = store.getDebuggeeProcessId(debugSessionId);
  const client = store.client.value;
  const starting = (): boolean => store.profiler.value.cpu === "starting";
  if (ready && pid !== undefined && client?.isRunning() === true && starting()) {
    report("Attaching the sampler…");
    await attachToDebuggee(store, client, { pid, launching: starting });
  }
}

/** Whether every runtime the server advertises CPU for injects a cooperative sampler. */
function cpuIsCooperativeEverywhere(store: Store): boolean {
  const runtimes = (store.capabilities.value?.runtimes ?? []).filter((runtime) => runtime.cpu !== undefined);
  return runtimes.length > 0 && runtimes.every((runtime) => runtime.cpu?.cooperative === true);
}

/**
 * CPU profiling for a debug-launched session with no task ports and no
 * elevation prompt: inject the in-process sampler at the `stopOnEntry` pause.
 */
async function startCooperativeProfileOnLaunch(store: Store, report: (message: string) => void): Promise<void> {
  const client = store.client.value;
  if (client?.isRunning() !== true) { return; }
  report("Waiting for the program to pause at entry…");
  const frameId = await waitForStoppedFrame();
  if (frameId === null) {
    warnCooperativeEntryMissing();
    return;
  }
  await injectCooperativeSamplerSafely(store, { client, frameId, report });
}

/** The pieces one cooperative injection needs, kept as one parameter. */
interface CooperativeInject {
  readonly client: LanguageClient;
  readonly frameId: number;
  readonly report: (message: string) => void;
}

/** A fetched cooperative script plus the sample rate it was generated for. */
interface CooperativeScriptRequest {
  readonly script: CooperativeScript;
  readonly sampleRate: number;
}

/** Tell the user why the sampler could not be injected. */
function warnCooperativeEntryMissing(): void {
  vscode.window.showWarningMessage(
    `${brand()}: The debuggee did not pause at entry, so the profiler could not be injected.`,
  );
}

/** Run the injection and turn a failure into the shared failure report. */
async function injectCooperativeSamplerSafely(store: Store, context: CooperativeInject): Promise<void> {
  try {
    await injectCooperativeSampler(store, context);
  } catch (error: unknown) {
    reportFailure("Cooperative profile start", error);
  }
}

/** Evaluate the sampler script at the entry pause, resume, then adopt its sample stream. */
async function injectCooperativeSampler(store: Store, context: CooperativeInject): Promise<void> {
  const requested = await requestCooperativeScript(context);
  if (requested === undefined) { return; }
  context.report("Injecting the in-process sampler…");
  const ack = await evaluateInDebugSession(requested.script.script, context.frameId);
  await vscode.commands.executeCommand("workbench.action.debug.continue");
  if (ack?.includes(COOPERATIVE_ACK) !== true) {
    vscode.window.showWarningMessage(`${brand()}: Could not inject the in-process CPU sampler.`);
    return;
  }
  await startCooperativeStream(store, context, requested);
}

/** Ask the server for the injection script, or undefined when it answered without one. */
async function requestCooperativeScript(context: CooperativeInject): Promise<CooperativeScriptRequest | undefined> {
  const sampleRate = samplerSettings().sample_rate;
  const script = await executeServerCommand<CooperativeScript>(context.client, CPU_CMD.cooperativeScript, { sample_rate: sampleRate });
  return script?.script === undefined ? undefined : { script, sampleRate };
}

/** Attach the in-process sample stream and adopt the session it starts. */
async function startCooperativeStream(store: Store, context: CooperativeInject, requested: CooperativeScriptRequest): Promise<void> {
  context.report("Starting the sample stream…");
  const raw = await executeServerCommand<StartProfileResult>(context.client, CPU_CMD.cooperativeAttach,
    { sample_file: requested.script.sample_file, sample_rate: requested.sampleRate });
  if (store.profiler.value.cpu === "active") { return; }
  adoptSession(store, raw, (result) => `${brand()}: Profiling current file (PID ${String(result.pid)}, in-process sampler)`);
}
