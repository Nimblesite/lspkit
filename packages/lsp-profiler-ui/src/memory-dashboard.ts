// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Memory dashboard webview for memory profiling.
 *
 * Provides:
 * - Summary cards: current memory, peak memory, gc objects, gc counts
 * - Timeline chart: memory usage over time (Canvas 2D line chart)
 * - Top allocations table: file, line, size, count — click to navigate
 * - Leak confidence badges: Definite, High, Medium, Low
 * - Dual heat map mode toggle: CPU or memory heat, in the host palette
 *
 * All data comes from the LSP via memory snapshot/diff commands.
 * This module handles only the client-side visualization.
 */

import * as vscode from "vscode";
import { brand, cmd } from "./ids";
import { Logger } from "./logger";
import type { MemoryAllocation } from "./memory-decorations";
import { openNativeTraceViewer, openSpeedscopeImport } from "./trace-actions";
import { formatBytes as formatBytesShared } from "./profiler-styles";
import { buildMemoryDashboardHtml } from "./memory-dashboard-html";
import { SingletonWebviewPanel, handleSourceNavigation } from "./profiler-webview";
import type { WebviewMessage } from "./profiler-webview";

export { buildMemoryDashboardHtml } from "./memory-dashboard-html";

// ── Types ─────────────────────────────────────────────────────────────────

/** A single time-series data point for the memory timeline. */
export interface MemoryTimelinePoint {
  timestamp: number;
  currentMemory: number;
  peakMemory: number;
  gcObjects: number;
}

/** Snapshot data extended with timeline and gc info. */
export interface MemoryDashboardSnapshot {
  memorySessionId: string;
  snapshotId: string;
  currentMemory: number;
  peakMemory: number;
  gcObjects: number;
  gcCounts: number[];
  /** Runtime-named heap regions measured in bytes, separate from GC counts. */
  generationSizes?: { name: string; size: number }[];
  topAllocations: MemoryAllocation[];
  timeline: MemoryTimelinePoint[];
  /** On-disk V8 `.heapprofile` for the built-in viewer; empty when not written. */
  heapProfilePath: string;
}

/** A suspected leak from a snapshot diff. */
export interface SuspectedLeak {
  file: string;
  line: number;
  sizeGrowth: number;
  countGrowth: number;
  currentSize: number;
  currentCount: number;
  confidence: "definite" | "high" | "medium" | "low";
  reason: string;
  /** The runtime's own name for the growing type, when it reports one. */
  typeName?: string;
}

/** Diff data comparing two memory snapshots. */
export interface MemoryDiffData {
  totalGrowth: number;
  totalFreed: number;
  netGrowth: number;
  suspectedLeaks: SuspectedLeak[];
  grownAllocations: MemoryAllocation[];
}

// ── State ─────────────────────────────────────────────────────────────────

// One panel, one message handler — the shared host guarantees a re-opened
// dashboard (the autopilot re-renders it on every pause) never stacks a second
// navigation handler ([PROF-UI-WEBVIEW]).
const memoryDashboardPanel = new SingletonWebviewPanel(() => cmd("memoryDashboard"), (msg) => {
  if (!handleMemoryDashboardMessage(msg)) {
    handleSourceNavigation(msg);
  }
});

/** The dashboard action buttons' message → command routing ([PROF-UI-MEMORY]). */
function dashboardActionCommand(type: string): string | undefined {
  switch (type) {
    case "takeSnapshot": { return cmd("memorySnapshot"); }
    case "compareSnapshots": { return cmd("memoryDiff"); }
    default: { return undefined; }
  }
}

/**
 * Route a heap-profile open message to its viewer; false when the message is
 * not one of the dashboard's heap-profile opens.
 */
function openHeapProfileFromMessage(msg: WebviewMessage): boolean {
  const file = msg.file ?? "";
  // The dashboard is the landing view; the raw V8 `.heapprofile` opens on demand
  // in VS Code's built-in viewer ([PROF-CPU-START]), or in speedscope, which
  // imports it too, over the CPU panel's loopback deep link ([PROF-UI-WEBVIEW]).
  if (msg.type === "openHeapProfile" && file !== "") {
    void openNativeTraceViewer(file, file);
    return true;
  }
  if (msg.type === "openSpeedscope" && file !== "") {
    void openSpeedscopeImport(file);
    return true;
  }
  return false;
}

/**
 * Route a dashboard action-button message to its real memory command, so the
 * dashboard's "take more snapshots" advice is a button, not homework
 * ([PROF-UI-MEMORY], #263). Returns whether the message was one of
 * the dashboard's actions (source-navigation clicks fall through).
 */
export function handleMemoryDashboardMessage(msg: WebviewMessage): boolean {
  if (openHeapProfileFromMessage(msg)) { return true; }
  const command = dashboardActionCommand(msg.type);
  if (command !== undefined) { void vscode.commands.executeCommand(command); }
  return command !== undefined;
}

// ── Public API ────────────────────────────────────────────────────────────

/**
 * Open or reveal the memory dashboard webview.
 * If diffData is provided, the leak analysis section is shown.
 */
export function openMemoryDashboard(
  snapshotData: MemoryDashboardSnapshot,
  diffData?: MemoryDiffData,
): void {
  memoryDashboardPanel.show(
    `${brand()} Memory Dashboard`,
    buildMemoryDashboardHtml(snapshotData, diffData),
  );
  const leakCount = diffData?.suspectedLeaks.length ?? 0;
  Logger.info(
    `Memory dashboard opened: ${formatBytesShared(snapshotData.currentMemory)} current, ` +
    `${String(snapshotData.topAllocations.length)} allocations, ${String(leakCount)} suspected leaks`,
  );
}

/** Dispose the memory dashboard panel if open. */
/** The open memory dashboard's view type, title and HTML (e2e seam). */
export function memoryDashboardState(): { readonly viewType: string; readonly title: string; readonly html: string } | undefined {
  const described = memoryDashboardPanel.describe();
  return described === undefined ? undefined : { ...described, html: memoryDashboardPanel.html() ?? "" };
}

export function disposeMemoryDashboard(): void {
  memoryDashboardPanel.dispose();
}
