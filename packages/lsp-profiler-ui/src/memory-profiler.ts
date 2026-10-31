// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Memory profiler command + lifecycle layer for the lsp-profiler UI library.
 *
 * Owns the `<prefix>.profiler.memory*` command registrations and the memory
 * status-bar indicator. Starting lives in [`memory-start.ts`](./memory-start.ts),
 * stopping and the at-exit finalisation ([PROF-PY-MEMORY-FINAL]) in
 * [`memory-finalize.ts`](./memory-finalize.ts), the editor-as-courier round-trip
 * and result presentation in [`memory-capture.ts`](./memory-capture.ts), the
 * reference-graph type picker in [`memory-ref-picker.ts`](./memory-ref-picker.ts)
 * and the automatic per-pause / interval capture in
 * [`memory-autopilot.ts`](./memory-autopilot.ts). All memory analysis logic lives
 * in the LSP — this module is client-side UI and command routing.
 */

import type * as vscode from "vscode";
import type { ProfilerStore as Store } from "./store";
import { bindMemoryStatusBar } from "./memory-status";
import { disposeMemoryDashboard } from "./memory-dashboard";
import { disposeRefGraph } from "./memory-ref-graph";
import { clearMemoryDecorations, disposeMemoryDecorations } from "./memory-decorations";
import { resetCaptureState } from "./memory-capture";
import { clearPendingSnapshotCleanup } from "./memory-finalize";
import { memoryCommandDisposables } from "./memory-registration";

// Re-exported so tests keep one import site for the memory profiler's seams.
export { memoryStatusText } from "./memory-status";
export { gcSummary } from "./memory-inspect-commands";

// ── State ─────────────────────────────────────────────────────────────────
//
// Memory-tracking session state lives in the store as a reactive signal
// ([PROF-UI-PROCESSES]); `boundStore` is the handle this module reads
// it through (and the e2e seam reads it through `activeMemorySession`).

let boundStore: Store | null = null;

// ── Registration ──────────────────────────────────────────────────────────

/** Register memory profiler commands and UI components. */
export function registerMemoryProfiler(store: Store): vscode.Disposable[] {
  boundStore = store;
  return [
    // The memory status-bar item — its whole lifecycle (idle affordance,
    // starting spinner, tracking readout) lives in memory-status.ts.
    ...bindMemoryStatusBar(store),
    ...memoryCommandDisposables(store),
  ];
}

/** Whether memory tracking is currently live, per the store. */
function isTracking(): boolean {
  return boundStore?.profiler.value.memory === "active";
}

/**
 * The active memory-tracking session id, or undefined when not tracking.
 * E2e seam for [PROF-UI-MEMORY]/[PROF-UI-PROCESSES]: lets
 * tests observe that tracking really started (e.g. the track-on-launch flow).
 */
export function activeMemorySession(): string | undefined {
  return isTracking() ? boundStore?.profiler.value.memorySessionId : undefined;
}

/** Clean up memory profiler resources. */
export function disposeMemoryProfiler(): void {
  clearMemoryDecorations();
  disposeMemoryDecorations();
  disposeRefGraph();
  disposeMemoryDashboard();
  resetCaptureState();
  clearPendingSnapshotCleanup();
}
