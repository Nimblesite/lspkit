// Implements [PROF-UI-MEMORY] + [PROF-UI-AUTOPILOT]. See docs/specs/LSP-PROFILER-SPEC.md
import { Logger } from "./logger";
import type { ProfilerStore as Store } from "./store";
import type { MemoryOperation } from "./memory-capture-types";
import { applyLeakDecorations, applyMemoryDecorations } from "./memory-decorations";
import type { MemoryDiffResult } from "./memory-decorations";
import type { MemoryIngestResult } from "./memory-dashboard-mapping";
import { toDashboardDiff, toDashboardSnapshot, toDiffResult, toSnapshotResult } from "./memory-dashboard-mapping";
import { openMemoryDashboard } from "./memory-dashboard";
import { beginMemoryOp, currentDashboardSnapshot, currentTimeline, endMemoryOp, recordTimelinePoint, setDashboardSnapshot } from "./memory-capture-state";
import { runMemoryOperation } from "./memory-capture-roundtrip";
import { LSP_MEM_CMD } from "./memory-capture-ids";

/**
 * Land a snapshot result: paint the allocation track, append a timeline
 * point, and retain it for a later "Compare". With `openResultsView` (the
 * manual "Take Memory Snapshot" affordance), open the memory dashboard
 * — the raw V8 `.heapprofile` stays one click away on its own button
 * ([PROF-CPU-START]). Without it (the autopilot's quiet per-pass capture) only
 * the decorations + timeline update — the diff step surfaces the dashboard.
 */
export function presentSnapshot(result: MemoryIngestResult, options: { openResultsView: boolean }): void {
  applyMemoryDecorations(toSnapshotResult(result));
  const dashboard = toDashboardSnapshot(result);
  recordTimelinePoint(dashboard);
  dashboard.timeline = [...currentTimeline()];
  setDashboardSnapshot(dashboard);
  Logger.info(`Memory snapshot: ${String(dashboard.currentMemory)} bytes current`);
  if (options.openResultsView) {
    openMemoryDashboard(dashboard);
  }
}

/**
 * Land a diff result: paint the leak decorations (confidence-coloured) and refresh
 * the dashboard with the leak analysis alongside the last snapshot. Returns the
 * typed diff so callers (the autopilot) can read suspected-leak confidence.
 */
export function presentDiff(result: MemoryIngestResult): MemoryDiffResult {
  const diff = toDiffResult(result);
  applyLeakDecorations(diff);
  const lastSnapshot = currentDashboardSnapshot();
  if (lastSnapshot !== null) {
    openMemoryDashboard(lastSnapshot, toDashboardDiff(result));
  }
  const leaks = diff.suspectedLeaks;
  Logger.info(`Memory diff: ${String(leaks.length)} suspected leak(s)`);
  return diff;
}

/** The result of one combined autopilot capture. */
export interface CaptureResult {
  readonly snapshot: MemoryIngestResult | null;
  readonly diff: MemoryDiffResult | null;
}

/**
 * The autopilot's combined capture: a snapshot then a diff, presented quietly
 * (decorations + dashboard + timeline, no new `.heapprofile` tab on each pass).
 * Brackets the whole pair as one in-flight op so the transparent pauses it may
 * perform never trigger a second auto-capture ([PROF-UI-AUTOPILOT]).
 */
export async function captureSnapshotAndDiff(
  store: Store,
  report: (message: string) => void,
): Promise<CaptureResult> {
  const quietOp = (command: string): MemoryOperation => ({ store, command, extraArgs: {}, report, quiet: true });
  beginMemoryOp();
  try {
    const snapshotResult = await runMemoryOperation(quietOp(LSP_MEM_CMD.snapshot));
    if (snapshotResult?.kind === "snapshot") {
      presentSnapshot(snapshotResult, { openResultsView: false });
    }
    const diffResult = await runMemoryOperation(quietOp(LSP_MEM_CMD.diff));
    const diff = diffResult?.kind === "diff" ? presentDiff(diffResult) : null;
    return { snapshot: snapshotResult, diff };
  } finally {
    endMemoryOp();
  }
}
