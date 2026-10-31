// Implements [PROF-UI-MEMORY] + [PROF-UI-AUTOPILOT]. See docs/specs/LSP-PROFILER-SPEC.md
import type { MemoryDashboardSnapshot, MemoryTimelinePoint } from "./memory-dashboard";

/** Milliseconds per second (timeline x-axis is in seconds). */
const MS_PER_SECOND = 1000;

// ── In-flight guard ─────────────────────────────────────────────────────────
//
// [PROF-UI-AUTOPILOT] A capture transparently pauses a running
// program, which emits its own `stopped` event; the autopilot must NOT treat that
// (or any in-progress manual op) as a fresh user pause. Every operation brackets
// itself with begin/end, so `isMemoryOperationInFlight()` is true for the whole
// pause→evaluate→resume window — the one synchronous fact both layers agree on.

let memoryOpsInFlight = 0;

/** True while any memory round-trip (manual or auto) is mid-flight. */
export function isMemoryOperationInFlight(): boolean {
  return memoryOpsInFlight > 0;
}

export function beginMemoryOp(): void {
  memoryOpsInFlight += 1;
}

export function endMemoryOp(): void {
  memoryOpsInFlight = Math.max(0, memoryOpsInFlight - 1);
}

// ── Presentation state ──────────────────────────────────────────────────────

/** Most recent snapshot, so a later "Compare" can show it alongside the diff. */
let lastDashboardSnapshot: MemoryDashboardSnapshot | null = null;

/** The most recent memory snapshot the dashboard rendered, with its timeline (e2e seam). */
export function lastMemorySnapshot(): MemoryDashboardSnapshot | undefined {
  return lastDashboardSnapshot ?? undefined;
}

/**
 * Rolling timeline of every snapshot captured this session. The dashboard chart
 * comes alive across repeated (especially autopilot) captures — "watch the leak
 * grow" — instead of always reading "take multiple snapshots". Reset on stop.
 */
let captureTimeline: MemoryTimelinePoint[] = [];

/** Whether the active memory session's runtime reads its heap directly ([PROF-MEMORY-MODE]). */
let directSession = false;

/** Whether a snapshot has been captured (so "stop" can report honestly). */
export function hasCapturedSnapshot(): boolean {
  return lastDashboardSnapshot !== null;
}

/** Drop per-session presentation state (called when tracking stops). */
export function resetCaptureState(): void {
  lastDashboardSnapshot = null;
  captureTimeline = [];
  directSession = false;
}

/** Record the mode of the memory session that just started. */
export function setDirectMemorySession(direct: boolean): void {
  directSession = direct;
}

/** Whether the active memory session reads its heap directly, with no debugger pause. */
export function isDirectMemorySession(): boolean {
  return directSession;
}

/** The snapshot the dashboard is currently rendering, or null before the first. */
export function currentDashboardSnapshot(): MemoryDashboardSnapshot | null {
  return lastDashboardSnapshot;
}

/** Replace the snapshot the dashboard renders (the "Compare" baseline). */
export function setDashboardSnapshot(snapshot: MemoryDashboardSnapshot): void {
  lastDashboardSnapshot = snapshot;
}

/** The rolling timeline of snapshots this session. */
export function currentTimeline(): MemoryTimelinePoint[] {
  return captureTimeline;
}

/** Append a timeline point for a snapshot (seconds-epoch x-axis the chart reads). */
export function recordTimelinePoint(snapshot: MemoryDashboardSnapshot): void {
  captureTimeline.push({
    timestamp: Date.now() / MS_PER_SECOND,
    currentMemory: snapshot.currentMemory,
    peakMemory: snapshot.peakMemory,
    gcObjects: snapshot.gcObjects,
  });
}
