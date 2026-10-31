// Implements [PROF-UI-MEMORY], [PROF-UI-PROCESSES] (memory leg). See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { brand, debugType } from "./ids";
import { Logger } from "./logger";
import type { ProfilerStore as Store } from "./store";
import { waitForStoppedFrame } from "./dap-evaluate";
import { withUserProgress } from "./progress-ops";
import { memStartTitle } from "./memory-capture";
import { handleMemoryStart } from "./memory-start-core";
import { memoryIsDirectEverywhere, startDirectTrackingOnLaunch } from "./memory-launch-direct";

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
