// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The profiler command bodies: start (reveal the panel), stop, and snapshot.
 */

import * as vscode from "vscode";
import { brand, viewId } from "./ids";
import type { ProfilerStore as Store } from "./store";
import { reportFailure } from "./failure";
import { withUserProgress } from "./progress-ops";
import { executeServerCommand } from "./server-command";
import { toProfileResult } from "./wire-adapters";
import type { ProfileReport } from "./wire-generated";
import { CPU_CMD } from "./profiler-attach";
import { announceSnapshot, presentStopResult } from "./profiler-results";

type ReportResult = ReturnType<typeof toProfileResult>;

async function handleProfileStart(): Promise<void> {
  // [PROF-UI-PROCESSES] #62: the profiler no longer asks the user to
  // hand-type a PID (and there was never a real "auto-detect"). Reveal the
  // Processes panel so they can pick a process — or run & profile the
  // current file — and start profiling with one click.
  await vscode.commands.executeCommand(`${viewId()}.focus`);
  vscode.window.showInformationMessage(
    `${brand()}: Pick a process in the panel to profile, or run & profile the current file.`,
  );
}

/** The running client and the active CPU session, or a warning that there is none. */
interface ActiveSession {
  readonly client: NonNullable<Store["client"]["value"]>;
  readonly sessionId: string;
}

function activeSession(store: Store): ActiveSession | undefined {
  const client = store.client.value;
  const sessionId = store.profiler.value.cpuSessionId;
  if (client?.isRunning() !== true || sessionId === undefined) {
    vscode.window.showWarningMessage(`${brand()}: No active profiling session.`);
    return undefined;
  }
  return { client, sessionId };
}

/** The title and first progress step of a report request. */
interface ReportProgress {
  readonly title: string;
  readonly step: string;
}

/**
 * Ask the server for the active session's report under a progress
 * notification: collecting samples and writing artifacts takes a beat
 * ([PROF-UI-STATE]).
 */
async function collectReport(active: ActiveSession, command: string, progress: ReportProgress): Promise<ReportResult | undefined> {
  return withUserProgress(progress.title, async (report) => {
    report(progress.step);
    const raw = await executeServerCommand<ProfileReport>(active.client, command, { session_id: active.sessionId });
    return raw === undefined ? undefined : toProfileResult(raw);
  });
}

export async function handleProfileStop(store: Store): Promise<void> {
  const active = activeSession(store);
  if (active === undefined) { return; }
  try {
    const result = await collectReport(active, CPU_CMD.stop, { title: `${brand()}: Stopping profiler`, step: "Collecting results…" });
    store.profilerStopped();
    presentStopResult(result);
  } catch (error: unknown) {
    store.profilerStopped();
    reportFailure("Profile stop", error);
  }
}

export async function handleProfileSnapshot(store: Store): Promise<void> {
  const active = activeSession(store);
  if (active === undefined) { return; }
  try {
    const result = await collectReport(active, CPU_CMD.snapshot, { title: `${brand()}: Taking profile snapshot`, step: "Collecting samples so far…" });
    if (result !== undefined) { announceSnapshot(result); }
  } catch (error: unknown) {
    reportFailure("Profile snapshot", error);
  }
}

export { handleProfileStart };
