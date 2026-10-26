// Implements [PROF-UI-STATE]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Centralized user-visible progress for long-running profiler operations.
 *
 * Every multi-second profiling flow (CPU start/stop, memory operations,
 * panel refresh) runs through these wrappers so notification styling,
 * structured logging, and testability stay uniform. VS Code's progress UI is
 * not readable via the public API, so the wrappers also record a
 * begin/step/end **operation log** — the e2e seam tests assert against.
 */

import * as vscode from "vscode";
import { Logger } from "./logger";

/**
 * Cap so a long session cannot grow the log unbounded. A host runs all its
 * suites in one extension host, so the cap sits far above what a whole run
 * records: within a run the log is never trimmed and its length stays a cursor.
 */
const OPERATION_LOG_CAP = 10_000;

/** Chronological `begin:`/`step:`/`end:`-prefixed operation entries. */
const operationLog: string[] = [];

function record(entry: string): void {
  operationLog.push(entry);
  if (operationLog.length > OPERATION_LOG_CAP) {
    operationLog.splice(0, settledPrefix(operationLog.length - OPERATION_LOG_CAP / 2));
  }
}

/**
 * The longest prefix of at most `limit` entries that holds only whole
 * operations. Trimming inside one would orphan its `end:` and make the log
 * claim a notification closed that never opened.
 */
function settledPrefix(limit: number): number {
  let open = 0;
  let settled = 0;
  for (const [index, entry] of operationLog.slice(0, limit).entries()) {
    open += entry.startsWith("begin:") ? 1 : entry.startsWith("end:") ? -1 : 0;
    settled = open === 0 ? index + 1 : settled;
  }
  return settled;
}

/** The operation log (e2e seam): `begin:<title>`, `step:<title>:<msg>`, `end:<title>`. */
export function recordedOperations(): readonly string[] {
  return operationLog;
}

/** The `report` a task narrates its stages through: recorded, logged and shown. */
function stageReporter(title: string, progress: vscode.Progress<{ message?: string }>): (message: string) => void {
  return (message) => {
    record(`step:${title}:${message}`);
    Logger.debug(`[Progress] ${title} — ${message}`);
    progress.report({ message });
  };
}

/**
 * Run `task` under a user-facing progress notification. The task receives a
 * `report` callback for live stage messages ("Attaching…", "Collecting
 * results…"). The notification closes when the task settles — including on
 * throw, so a failed flow never leaves a zombie spinner.
 */
export async function withUserProgress<T>(
  title: string,
  task: (report: (message: string) => void) => Promise<T>,
): Promise<T> {
  record(`begin:${title}`);
  Logger.info(`[Progress] begin: ${title}`);
  try {
    return await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title },
      async (progress) => await task(stageReporter(title, progress)),
    );
  } finally {
    record(`end:${title}`);
    Logger.info(`[Progress] end: ${title}`);
  }
}

/**
 * Run `task` under a tree view's built-in progress bar (the thin indeterminate
 * bar atop the view) — the right surface for an in-panel refresh, where a
 * notification would be heavyweight.
 */
export async function withViewProgress<T>(
  viewId: string,
  title: string,
  task: () => Promise<T>,
): Promise<T> {
  record(`begin:${title}`);
  try {
    return await vscode.window.withProgress({ location: { viewId } }, task);
  } finally {
    record(`end:${title}`);
  }
}
