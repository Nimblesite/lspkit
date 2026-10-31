// Implements [PROF-UI-MEMORY], [PROF-UI-PROCESSES] (memory leg). See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { brand } from "./ids";
import { Logger } from "./logger";
import type { ProfilerStore as Store } from "./store";
import { acquireStoppedFrame, evaluateInDebugSession } from "./dap-evaluate";
import { executeServerCommand } from "./server-command";
import type { MemoryStartResult } from "./wire-generated";
import { LSP_MEM_CMD, TRACEBACK_DEPTH, setDirectMemorySession } from "./memory-capture";

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

/** Resolve the client and debug target one memory start runs against. */
function startContext(store: Store, pid?: number): StartTrackingContext | undefined {
  const client = clientForMemoryStart(store);
  if (client === undefined) { return undefined; }
  const debugSession = vscode.debug.activeDebugSession;
  const target = memoryTarget(store, debugSession, pid);
  if (target === undefined) { return undefined; }
  return { client, target: { target, debugSession } };
}

/**
 * Start tracking and adopt the session. Returns whether tracking actually
 * started, so each caller shows its own context-appropriate message. The start
 * reply carries the at-exit `finalSnapshotFile` the session is finalised from on
 * session end ([PROF-PY-MEMORY-FINAL]).
 */
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
