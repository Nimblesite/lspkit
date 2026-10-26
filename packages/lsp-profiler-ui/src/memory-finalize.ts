// Implements [PROF-UI-MEMORY], [PROF-PY-MEMORY-FINAL]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Ending memory tracking: a manual stop, and the tracked debug session's end,
 * which turns the run into a visible result. Stopping never silently produces
 * nothing (#146).
 */

import * as vscode from "vscode";
import { brand, cmd, debugType } from "./ids";
import { Logger } from "./logger";
import type { ProfilerStore as Store } from "./store";
import { errorText } from "./failure";
import { openMemoryDashboard } from "./memory-dashboard";
import { clearMemoryDecorations } from "./memory-decorations";
import {
  endServerSession,
  ingestOutput,
  isDirectMemorySession,
  lastMemorySnapshot,
  presentSnapshot,
  readFinalSnapshot,
  resetCaptureState,
} from "./memory-capture";

/**
 * Final-snapshot files awaiting cleanup, keyed by debug-session id. When the
 * user stops tracking *mid-run*, the debuggee's `atexit` hook is still armed and
 * will write its file when the program eventually exits; it is deleted on that
 * session's termination so a manual stop never orphans a temp file
 * ([PROF-PY-MEMORY-FINAL]).
 */
const pendingSnapshotCleanup = new Map<string, string>();

/** Forget every pending cleanup (disposal). */
export function clearPendingSnapshotCleanup(): void {
  pendingSnapshotCleanup.clear();
}

/** Schedule the tracked session's at-exit file for deletion when that session ends. */
export function scheduleSnapshotCleanup(store: Store): void {
  const debugSessionId = store.profiler.value.memoryDebugSessionId;
  const finalSnapshotFile = store.profiler.value.memoryFinalSnapshotFile;
  if (debugSessionId !== undefined && finalSnapshotFile !== undefined) {
    pendingSnapshotCleanup.set(debugSessionId, finalSnapshotFile);
  }
}

/**
 * Show a toast whose named action is offered as a button — a message must
 * never demand an action it doesn't offer ([PROF-UI-MEMORY], #263).
 * Fire-and-forget: a sticky notification must not block the flow.
 */
export function showActionableToast(message: string, action: string, command: string): void {
  void vscode.window.showInformationMessage(message, action).then((choice) => {
    if (choice === action) {
      void vscode.commands.executeCommand(command);
    }
  });
}

/** Settle the UI's tracking state to idle. */
export function handleMemoryStop(store: Store): void {
  store.memoryTrackingStopped();
  resetCaptureState();
  clearMemoryDecorations();
  Logger.info("Memory tracking stopped");
}

/**
 * A debug session ended. Only the *tracked* session's end finalises the run
 * ([PROF-PY-MEMORY-FINAL], #146); an unrelated session never tears down live
 * tracking. A manually stopped session deletes its orphaned at-exit file.
 */
export function settleMemoryOnSessionEnd(store: Store, session: vscode.DebugSession): void {
  if (store.profiler.value.memory !== "idle" && isTrackedSession(store, session)) {
    pendingSnapshotCleanup.delete(session.id);
    void finalizeMemorySessionOnEnd(store);
    return;
  }
  const orphan = pendingSnapshotCleanup.get(session.id);
  if (orphan !== undefined) {
    pendingSnapshotCleanup.delete(session.id);
    void readFinalSnapshot(orphan); // reads + unlinks; the payload is discarded
  }
}

/**
 * Whether `session` is the tracked one. Without a captured session id
 * (`activeDebugSession` was momentarily undefined at start, memui-3), any
 * host-debug-type session is: only one memory session exists at a time, so
 * tracking always settles instead of hanging on "Memory: tracking".
 */
function isTrackedSession(store: Store, session: vscode.DebugSession): boolean {
  const trackedId = store.profiler.value.memoryDebugSessionId;
  return trackedId !== undefined ? session.id === trackedId : session.type === debugType();
}

/**
 * Finalise a courier session when its debug session ends. A run with no
 * breakpoint has no paused frame to snapshot from, so the start script
 * registered an `atexit` hook that wrote a final snapshot to a file. The
 * tracking state is torn down first so the panel settles, then that file is
 * ingested and presented exactly like a manual snapshot.
 */
async function finalizeMemorySessionOnEnd(store: Store): Promise<void> {
  if (isDirectMemorySession()) {
    finishDirectRunOnEnd(store);
    return;
  }
  const { memorySessionId, memoryFinalSnapshotFile: finalSnapshotFile } = store.profiler.value;
  handleMemoryStop(store);
  if (memorySessionId === undefined || finalSnapshotFile === undefined) { return; }
  const output = await readFinalSnapshot(finalSnapshotFile);
  if (output === null) {
    // No tracked allocations, or an exit too abrupt for even the SIGTERM/SIGINT
    // hook (a native crash, SIGKILL, os._exit). Never claim it "finished" (ux-1).
    void vscode.window.showInformationMessage(
      `${brand()}: No final memory snapshot was captured — the program either made no tracked allocations or exited abruptly (a crash, kill, or os._exit). Set a breakpoint and take a snapshot to inspect allocations.`,
    );
    return;
  }
  await presentFinalSnapshot(store, memorySessionId, output);
}

/** A final snapshot was captured at exit, `but` something then went wrong. */
function warnCapturedBut(but: string): void {
  void vscode.window.showWarningMessage(`${brand()}: Captured a final memory snapshot at exit, but ${but}.`);
}

/**
 * The snapshot WAS captured: every failure to turn it into a view is surfaced,
 * never swallowed (memui-2).
 */
async function presentFinalSnapshot(store: Store, memorySessionId: string, output: string): Promise<void> {
  const client = store.client.value;
  if (client?.isRunning() !== true) {
    warnCapturedBut("the language server is not running, so it could not be analyzed");
    return;
  }
  try {
    const result = await ingestOutput(client, memorySessionId, output);
    if (result?.kind !== "snapshot") {
      warnCapturedBut("it could not be analyzed into an allocation view");
      return;
    }
    presentSnapshot(result, { openResultsView: true });
    void vscode.window.showInformationMessage(`${brand()}: Captured a final memory snapshot at exit — opened the allocation view.`);
  } catch (err: unknown) {
    Logger.warn(`[Memory] final snapshot ingest failed: ${errorText(err)}`);
    warnCapturedBut(`analyzing it failed — ${errorText(err)}`);
  }
}

/**
 * A direct runtime's heap is gone once its program has ended, so the run's
 * result is the last snapshot taken while it ran: open it, never nothing
 * ([PROF-PY-MEMORY-FINAL], #146).
 */
function finishDirectRunOnEnd(store: Store): void {
  const last = lastMemorySnapshot();
  void endServerSession(store);
  handleMemoryStop(store);
  if (last === undefined) {
    showActionableToast(
      `${brand()}: The tracked program ended before a memory snapshot was taken.`,
      "Memory Actions…",
      cmd("memoryMenu"),
    );
    return;
  }
  openMemoryDashboard(last);
  void vscode.window.showInformationMessage(`${brand()}: The tracked program ended — opened its last memory snapshot.`);
}
