// Implements [PROF-UI-MEMORY], [PROF-UI-PROCESSES] (memory leg). See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Starting memory tracking: from the menu, from a Processes row and from a
 * "Run & Track Memory (Current File)" launch (#82).
 */

import * as vscode from "vscode";
import { brand, debugType } from "./ids";
import { Logger } from "./logger";
import type { ProfilerStore as Store } from "./store";
import { acquireStoppedFrame, evaluateInDebugSession, waitForStoppedFrame } from "./dap-evaluate";
import { withUserProgress } from "./progress-ops";
import { waitForDebuggeePid } from "./profiler-launch";
import { trackingGuidance } from "./memory-guidance";
import { captureIntervalSeconds } from "./memory-autopilot";
import { executeServerCommand } from "./server-command";
import type { MemoryStartResult } from "./wire-generated";
import {
  LSP_MEM_CMD,
  TRACEBACK_DEPTH,
  captureSnapshotAndDiff,
  memStartTitle,
  setDirectMemorySession,
} from "./memory-capture";

/** The running language client the store exposes, once one is live. */
type LspClient = NonNullable<Store["client"]["value"]>;

/**
 * The running client, when a memory start may begin. Memory starts gate on the
 * memory leg only ([PROF-UI-PROCESSES]): independent of any live CPU profile,
 * but never a second memory session. Otherwise the reason is shown.
 */
function clientForMemoryStart(store: Store): LspClient | undefined {
  const client = store.client.value;
  if (client?.isRunning() !== true) {
    void vscode.window.showErrorMessage(`${brand()} LSP not connected`);
    return undefined;
  }
  if (store.memoryBusy.value) {
    void vscode.window.showWarningMessage(`${brand()}: Memory tracking is already active.`);
    return undefined;
  }
  return client;
}

/** The process to track: an explicit PID (a Processes row) or the active debuggee. */
function memoryTarget(store: Store, debugSession: vscode.DebugSession | undefined, pid?: number): number | undefined {
  const target = pid ?? (debugSession === undefined ? undefined : store.getDebuggeeProcessId(debugSession.id));
  if (target === undefined) {
    void vscode.window.showWarningMessage(`${brand()}: Pick a process in the panel, or debug a program, to track its memory.`);
  }
  return target;
}

/**
 * Start tracking and adopt the session. Returns whether tracking actually
 * started, so each caller shows its own context-appropriate message. The start
 * reply carries the at-exit `finalSnapshotFile` the session is finalised from on
 * session end ([PROF-PY-MEMORY-FINAL]).
 */
/** Resolve the client and debug target one memory start runs against. */
function startContext(store: Store, pid?: number): StartTrackingContext | undefined {
  const client = clientForMemoryStart(store);
  if (client === undefined) { return undefined; }
  const debugSession = vscode.debug.activeDebugSession;
  const target = memoryTarget(store, debugSession, pid);
  if (target === undefined) { return undefined; }
  return { client, target: { target, debugSession } };
}

export async function handleMemoryStart(store: Store, report: (message: string) => void, pid?: number): Promise<boolean> {
  const context = startContext(store, pid);
  if (context === undefined) { return false; }
  // From here on the panel reflects a tracking start in flight; every exit path
  // must settle it back to active or idle ([PROF-UI-STATE]).
  store.memoryTrackingStarting();
  try {
    return await startTracking(store, context, report);
  } catch (error) {
    return reportStartFailure(store, context.target.target, error);
  }
}

/** Settle a failed start back to idle and tell the user why. */
function reportStartFailure(store: Store, target: number, error: unknown): false {
  store.memoryTrackingStopped();
  const message = error instanceof Error ? error.message : String(error);
  Logger.warn(`Memory tracking failed for PID ${String(target)}: ${message}`);
  void vscode.window.showErrorMessage(`Memory tracking failed: ${message}`);
  return false;
}

/** The process a start tracks, and the debug session that was active when it began. */
interface MemoryStartTarget {
  readonly target: number;
  readonly debugSession: vscode.DebugSession | undefined;
}

/** The client and debug target one memory start runs against. */
interface StartTrackingContext {
  readonly client: LspClient;
  readonly target: MemoryStartTarget;
}

/** Ask the server to start, run a courier runtime's script in the debuggee, then adopt. */
async function startTracking(store: Store, context: StartTrackingContext, report: (message: string) => void): Promise<boolean> {
  report("Starting the memory session…");
  const result = await executeServerCommand<MemoryStartResult>(context.client, LSP_MEM_CMD.start,
    { pid: context.target.target, traceback_depth: TRACEBACK_DEPTH });
  if (result === undefined || (result.step.kind === "Evaluate" && !(await injectCourier(result.step.script, report)))) {
    store.memoryTrackingStopped();
    return false;
  }
  adoptMemorySession(store, context, result);
  return true;
}

/** Adopt the started session: remember its debug session, mode and live id. */
function adoptMemorySession(store: Store, context: StartTrackingContext, result: MemoryStartResult): void {
  // Remember which debug session this tracks, so only its termination
  // finalises the run ([PROF-PY-MEMORY-FINAL]).
  const session = context.target.debugSession;
  const tracked = store.getDebuggeeProcessId(session?.id ?? "") === context.target.target ? session?.id : undefined;
  store.memoryTrackingActive(result.memory_session_id, undefined, tracked);
  setDirectMemorySession(result.mode.kind === "Direct");
  Logger.info(`Memory tracking started: session ${result.memory_session_id} (${result.mode.kind})`);
}

/**
 * Courier runtimes ([PROF-MEMORY-MODE]) start tracking by running a script in
 * the paused debuggee: pause transparently, inject, resume.
 */
async function injectCourier(script: string, report: (message: string) => void): Promise<boolean> {
  report("Pausing the program…");
  const acquired = await acquireStoppedFrame();
  if (acquired === null) {
    warnPauseFailed();
    return false;
  }
  try {
    return await injectTracker(script, acquired.frameId, report);
  } finally {
    await acquired.release();
  }
}

/** Evaluate the courier script in an acquired frame and report the outcome. */
async function injectTracker(script: string, frameId: number, report: (message: string) => void): Promise<boolean> {
  report("Injecting the tracker…");
  const injected = (await evaluateInDebugSession(script, frameId)) !== null;
  if (!injected) { warnInjectionFailed(); }
  return injected;
}

function warnPauseFailed(): void {
  void vscode.window.showWarningMessage(
    `${brand()}: Could not pause the program — pause at a breakpoint, then start memory tracking.`,
  );
}

function warnInjectionFailed(): void {
  void vscode.window.showWarningMessage(`${brand()}: Could not start the memory tracker in the debuggee.`);
}

/** "Run & Track Memory (Current File)" (#82): track a launch that asked for it. */
export function trackMemoryOnLaunch(store: Store, session: vscode.DebugSession): void {
  if (session.type === debugType() && session.configuration.memoryTrackOnLaunch === true) {
    void startMemoryTrackingOnLaunch(store, session);
  }
}

/**
 * Memory tracking on a courier runtime needs a paused debuggee
 * ([PROF-UI-MEMORY]), so the launch config sets `stopOnEntry`; the tracker is
 * injected at the entry pause and the debuggee resumed. With a breakpoint set,
 * the autopilot then captures each pass ([PROF-UI-AUTOPILOT]); with none, a
 * final snapshot opens at exit ([PROF-PY-MEMORY-FINAL]).
 */
async function startMemoryTrackingOnLaunch(store: Store, session: vscode.DebugSession): Promise<void> {
  if (memoryIsDirectEverywhere(store)) {
    await startDirectTrackingOnLaunch(store, session);
    return;
  }
  // One notification spans the whole auto-start ([PROF-UI-STATE]).
  await withUserProgress(memStartTitle(), async (report) => { await trackFromEntryPause(store, report); });
}

/** Start tracking at the launch's entry pause, then resume the program. */
async function trackFromEntryPause(store: Store, report: (message: string) => void): Promise<void> {
  report("Waiting for the program to pause at entry…");
  if ((await waitForStoppedFrame()) === null) {
    void vscode.window.showWarningMessage(
      `${brand()}: The debuggee did not pause at entry — pause at a breakpoint, then start memory tracking.`,
    );
    return;
  }
  if (!(await handleMemoryStart(store, report))) { return; }
  Logger.info("Memory tracking started on launch — resuming the debuggee");
  void vscode.window.showInformationMessage(launchTrackingMessage());
  report("Resuming the program…");
  await vscode.commands.executeCommand("workbench.action.debug.continue");
}

/**
 * The outcome, stated up front so the toast never points nowhere: a run with a
 * breakpoint auto-captures each pass ([PROF-UI-AUTOPILOT]); one without
 * finalises at exit ([PROF-PY-MEMORY-FINAL], #146).
 */
function launchTrackingMessage(): string {
  return vscode.debug.breakpoints.length > 0
    ? `${brand()}: Tracking memory — press Continue and each pause is captured automatically.`
    : `${brand()}: Tracking memory — a final snapshot opens automatically when the program finishes.`;
}

/** Whether every runtime the server advertises memory for reads its heap directly. */
function memoryIsDirectEverywhere(store: Store): boolean {
  const runtimes = (store.capabilities.value?.runtimes ?? []).filter((runtime) => runtime.memory !== undefined);
  return runtimes.length > 0 && runtimes.every((runtime) => runtime.memory?.mode.kind === "Direct");
}

/**
 * "Run & Track Memory" for a direct runtime ([PROF-MEMORY-MODE]): nothing has
 * to pause, so tracking starts as soon as the debuggee reports its process, and
 * a baseline snapshot is taken so the run always has a result to show (#146).
 * Each pass after that is captured when the program continues
 * ([PROF-UI-AUTOPILOT]).
 */
async function startDirectTrackingOnLaunch(store: Store, session: vscode.DebugSession): Promise<void> {
  await withUserProgress(memStartTitle(), async (report) => {
    report("Waiting for the program to start…");
    const pid = (await waitForDebuggeePid(store, session.id)) ? store.getDebuggeeProcessId(session.id) : undefined;
    if (pid === undefined) {
      void vscode.window.showWarningMessage(
        `${brand()}: The program never reported its process — track its memory from its row in the Processes panel.`,
      );
      return;
    }
    if (!(await handleMemoryStart(store, report, pid))) { return; }
    await captureSnapshotAndDiff(store, report);
    void vscode.window.showInformationMessage(trackingGuidance(true, captureIntervalSeconds()));
  });
}
