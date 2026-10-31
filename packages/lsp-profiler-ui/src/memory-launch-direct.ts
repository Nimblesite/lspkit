// Implements [PROF-UI-MEMORY], [PROF-UI-PROCESSES] (memory leg). See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { brand } from "./ids";
import type { ProfilerStore as Store } from "./store";
import { withUserProgress } from "./progress-ops";
import { waitForDebuggeePid } from "./profiler-launch";
import { trackingGuidance } from "./memory-guidance";
import { captureIntervalSeconds } from "./memory-autopilot";
import { captureSnapshotAndDiff, memStartTitle } from "./memory-capture";
import { handleMemoryStart } from "./memory-start-core";

/** Whether every runtime the server advertises memory for reads its heap directly. */
export function memoryIsDirectEverywhere(store: Store): boolean {
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
export async function startDirectTrackingOnLaunch(store: Store, session: vscode.DebugSession): Promise<void> {
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
