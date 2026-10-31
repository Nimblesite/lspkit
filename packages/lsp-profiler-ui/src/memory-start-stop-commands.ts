// Implements [PROF-UI-MEMORY]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { brand, cmd } from "./ids";
import type { ProfilerStore as Store } from "./store";
import { endServerSession, hasCapturedSnapshot, isDirectMemorySession } from "./memory-capture";
import { handleMemoryStop, scheduleSnapshotCleanup, showActionableToast } from "./memory-finalize";
import { stoppedWithoutSnapshot, trackingGuidance } from "./memory-guidance";
import { captureIntervalSeconds } from "./memory-autopilot";
import { handleMemoryStart } from "./memory-start-core";

/** The `<prefix>.profiler.memoryStart` body: start tracking, then narrate what comes next. */
export async function runMemoryStartCommand(store: Store, report: (message: string) => void, pid?: number): Promise<void> {
  if (await handleMemoryStart(store, report, pid)) {
    // The moment of disorientation is right here (the user just landed in the
    // Debug view), so the toast says what happens next in the tracked runtime's
    // own mode ([PROF-UI-AUTOPILOT]) and carries the action ([PROF-UI-MEMORY], #263).
    showActionableToast(
      trackingGuidance(isDirectMemorySession(), captureIntervalSeconds()),
      "Take Snapshot",
      cmd("memorySnapshot"),
    );
  }
}

/**
 * The `<prefix>.profiler.memoryStop` body. Stopping never silently produces
 * nothing: it says whether a snapshot was captured, or plainly that none was
 * ([PROF-PY-MEMORY-FINAL], #146). Stopping mid-run leaves the debuggee's
 * `atexit` hook armed, so the file it will write is scheduled for cleanup.
 */
export async function runMemoryStopCommand(store: Store): Promise<void> {
  const hadSnapshot = hasCapturedSnapshot();
  const direct = isDirectMemorySession();
  scheduleSnapshotCleanup(store);
  await endServerSession(store);
  handleMemoryStop(store);
  if (hadSnapshot) {
    void vscode.window.showInformationMessage(`${brand()}: Memory tracking stopped.`);
    return;
  }
  // The toast tells the user to take a snapshot, so it offers the way there
  // (the quick-pick action menu), never an invisible palette ([PROF-UI-MEMORY], #263).
  showActionableToast(stoppedWithoutSnapshot(direct), "Memory Actions…", cmd("memoryMenu"));
}
