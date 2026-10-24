// Implements [PROF-UI-MEMORY] + [PROF-UI-AUTOPILOT].
// See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The editor-as-courier memory round-trip and result presentation.
 *
 * Extracted from memory-profiler.ts (which was over the 500 LOC limit) so the
 * command/lifecycle layer and the capture engine each stay small and focused.
 * Both the manual commands ([`memory-profiler.ts`](./memory-profiler.ts)) and the
 * autopilot ([`memory-autopilot.ts`](./memory-autopilot.ts)) drive memory through
 * the same primitives here, so an auto-capture is byte-for-byte the same flow as a
 * hand-clicked one — same allocation track, leak decorations, dashboard, and timeline.
 *
 * Round-trip (one operation): ask the LSP for the injection script (leg 1), run it
 * in the paused debuggee via DAP `evaluate`, post the raw output to
 * `<prefix>.lspProfiler.memory.ingest` (leg 2), and present the structured result. A running
 * program is transparently paused for the script and resumed after
 * ([PROF-UI-MEMORY]); a user's own breakpoint pause is left untouched.
 */

import { delay } from "./timeouts";
import { lspTable, brand } from "./ids";
import * as vscode from "vscode";
import * as fs from "fs";
import { Logger } from "./logger";
import { errorText } from "./failure";
import { executeServerCommand } from "./server-command";
import type { ProfilerStore as Store } from "./store";
import { acquireStoppedFrame, evaluateInDebugSession } from "./dap-evaluate";
import { isDebuggeePaused } from "./debuggee-pause";
import { withUserProgress } from "./progress-ops";
import { toMemoryIngestResult } from "./wire-adapters";
import type { MemoryResult, MemoryStep } from "./wire-generated";
import {
  toDashboardDiff,
  toDashboardSnapshot,
  toDiffResult,
  toSnapshotResult,
  asNumber,
  type MemoryIngestResult,
} from "./memory-dashboard-mapping";
import {
  openMemoryDashboard,
  type MemoryDashboardSnapshot,
  type MemoryTimelinePoint,
} from "./memory-dashboard";
import {
  applyLeakDecorations,
  applyMemoryDecorations,
  type MemoryDiffResult,
} from "./memory-decorations";
// ── LSP command ids ─────────────────────────────────────────────────────────

/** The `<prefix>.lspProfiler.memory.*` LSP command names (one round-trip leg each). */
export const LSP_MEM_CMD = lspTable({
  start: "memory.start",
  snapshot: "memory.snapshot",
  diff: "memory.diff",
  references: "memory.references",
  objectsByType: "memory.objectsByType",
  gcCollect: "memory.gcCollect",
  ingest: "memory.ingest",
  stop: "memory.stop",
});

/** tracemalloc traceback depth injected at start. */
export const TRACEBACK_DEPTH = 25;

/** The progress title of a memory start; a function so it follows the installed brand. */
export function memStartTitle(): string {
  return `${brand()}: Starting memory tracking`;
}

/** Milliseconds per second (timeline x-axis is in seconds). */
const MS_PER_SECOND = 1000;

/** [PROF-UI-STATE] Progress-notification titles, one per memory operation. */
function memOpTitle(command: string): string {
  switch (command) {
    case LSP_MEM_CMD.snapshot: return `${brand()}: Taking memory snapshot`;
    case LSP_MEM_CMD.diff: return `${brand()}: Comparing memory snapshots`;
    case LSP_MEM_CMD.gcCollect: return `${brand()}: Forcing garbage collection`;
    case LSP_MEM_CMD.references: return `${brand()}: Building the reference graph`;
    default: return `${brand()}: Inspecting memory`;
  }
}

/** [PROF-PY-MEMORY-FINAL] How long to wait for the at-exit snapshot file after the
 *  program exits (covers the terminate-event/final-flush race), and the poll
 *  cadence while waiting. */
const FINAL_SNAPSHOT_WAIT_MS = 3000;
const FINAL_SNAPSHOT_POLL_MS = 100;

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

function beginMemoryOp(): void {
  memoryOpsInFlight += 1;
}

function endMemoryOp(): void {
  memoryOpsInFlight = Math.max(0, memoryOpsInFlight - 1);
}

// ── Presentation state ──────────────────────────────────────────────────────

/** Most recent snapshot, so a later "Compare" can show it alongside the diff. */
let lastDashboardSnapshot: MemoryDashboardSnapshot | undefined;

/** The most recent memory snapshot the dashboard rendered, with its timeline (e2e seam). */
export function lastMemorySnapshot(): MemoryDashboardSnapshot | undefined {
  return lastDashboardSnapshot;
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
  return lastDashboardSnapshot !== undefined;
}

/** Drop per-session presentation state (called when tracking stops). */
export function resetCaptureState(): void {
  lastDashboardSnapshot = undefined;
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

// ── Single operation round-trip ─────────────────────────────────────────────

/** The running LSP client handle. */
type LspClient = NonNullable<Store["client"]["value"]>;

/**
 * End the tracked session on the server ([PROF-MEMORY-SESSION]): it answers no
 * further step and its heap graphs are deleted ([PROF-DOTNET-MEMORY] `stop`).
 * Call it while the store still holds the session id; a failure is logged,
 * because the UI's own tracking ends either way.
 */
export async function endServerSession(store: Store): Promise<void> {
  const client = store.client.value;
  const memorySessionId = store.profiler.value.memorySessionId;
  if (client?.isRunning() !== true || memorySessionId === undefined) { return; }
  try {
    await executeServerCommand<unknown>(client, LSP_MEM_CMD.stop, { memory_session_id: memorySessionId });
  } catch (err: unknown) {
    Logger.warn(`[Memory] ${LSP_MEM_CMD.stop} failed: ${errorText(err)}`);
  }
}

/** Everything one staged memory round-trip needs. */
interface MemoryOperation {
  readonly store: Store;
  readonly command: string;
  readonly extraArgs: Record<string, unknown>;
  readonly report: (message: string) => void;
  /** Suppress user-facing warnings (the autopilot captures silently). */
  readonly quiet: boolean;
}

/** The running client and the memory session it tracks. */
interface ActiveMemorySession {
  readonly client: LspClient;
  readonly memorySessionId: string;
}

/** Show `message` unless the operation captures silently. */
function warnUnlessQuiet(op: MemoryOperation, message: string): void {
  if (!op.quiet) { void vscode.window.showWarningMessage(message); }
}

/**
 * Resolve the running client + active memory session, or null (warning unless
 * `quiet`) when the LSP is down or tracking is not active.
 */
function resolveActiveSession(op: MemoryOperation): ActiveMemorySession | null {
  const client = op.store.client.value;
  function notConnected(): null {
    if (!op.quiet) { void vscode.window.showErrorMessage(`${brand()} LSP not connected`); }
    return null;
  }
  if (client === undefined) { return notConnected(); }
  if (!client.isRunning()) { return notConnected(); }
  const memorySessionId = op.store.profiler.value.memorySessionId;
  if (op.store.profiler.value.memory !== "active" || memorySessionId === undefined) {
    warnUnlessQuiet(op, `${brand()}: Start memory tracking first.`);
    return null;
  }
  return { client, memorySessionId };
}

/**
 * Run one memory operation's round-trip and return the LSP's structured result.
 *
 * Returns null (with an actionable message unless `quiet`) when there is no
 * session, the debuggee cannot be paused, or evaluation fails.
 */
async function runMemoryOperation(op: MemoryOperation): Promise<MemoryIngestResult | null> {
  const active = resolveActiveSession(op);
  if (active === null || refusedWhilePaused(op)) { return null; }
  beginMemoryOp();
  try {
    return await requestStep(op, active);
  } catch (err: unknown) {
    const msg = errorText(err);
    Logger.warn(`[Memory] ${op.command} round-trip failed: ${msg}`);
    warnUnlessQuiet(op, `${brand()}: ${msg}`);
    return null;
  } finally {
    endMemoryOp();
  }
}

/**
 * A direct runtime reads the live heap, which it cannot do while the debugger
 * holds the program paused: the step would wait for a resume that may never
 * come ([PROF-MEMORY-MODE]). Refuse it and say why.
 */
function refusedWhilePaused(op: MemoryOperation): boolean {
  if (!directSession || !isDebuggeePaused(op.store.profiler.value.memoryDebugSessionId)) { return false; }
  warnUnlessQuiet(op, `${brand()}: Continue the program to read its memory — its heap cannot be read while the debugger holds it paused.`);
  return true;
}

/** Leg 1: ask for the operation's step; a completed step is the result, a courier step runs in the debuggee. */
async function requestStep(op: MemoryOperation, active: ActiveMemorySession): Promise<MemoryIngestResult | null> {
  op.report("Asking the profiler…");
  const step = await executeServerCommand<MemoryStep>(active.client, op.command,
    { memory_session_id: active.memorySessionId, ...op.extraArgs });
  if (step === undefined) { return null; }
  if (step.kind === "Completed") { return toMemoryIngestResult(step.result); }
  return courierStep(op, active, step.script);
}

/**
 * A courier step ([PROF-MEMORY-MODE]): the runtime cannot read its own heap,
 * so the script runs inside the paused debuggee and its output is posted back.
 */
async function courierStep(op: MemoryOperation, active: ActiveMemorySession, script: string): Promise<MemoryIngestResult | null> {
  op.report("Pausing the program…");
  const acquired = await acquireStoppedFrame();
  if (acquired === null) {
    warnUnlessQuiet(op, `${brand()}: Could not pause the program for memory inspection — pause at a breakpoint and retry.`);
    return null;
  }
  try {
    return await evaluateAndIngest(op, { ...active, frameId: acquired.frameId }, script);
  } finally {
    await acquired.release();
  }
}

/** The acquired-frame context one round-trip evaluates against. */
interface OperationContext extends ActiveMemorySession {
  readonly frameId: number;
}

/** Leg 2: run the script in the frame, then post the output to ingest. */
async function evaluateAndIngest(op: MemoryOperation, ctx: OperationContext, script: string): Promise<MemoryIngestResult | null> {
  if (script === "") { return null; }
  op.report("Inspecting the debuggee…");
  const output = await evaluateInDebugSession(script, ctx.frameId);
  if (output === null) {
    warnUnlessQuiet(op, `${brand()}: Could not run the memory script in the debuggee.`);
    return null;
  }
  op.report("Analyzing…");
  return ingestOutput(ctx.client, ctx.memorySessionId, output);
}

/** Post a script's raw output to `<prefix>.lspProfiler.memory.ingest` and read the structured result. */
export async function ingestOutput(client: LspClient, memorySessionId: string, output: string): Promise<MemoryIngestResult | null> {
  const result = await executeServerCommand<MemoryResult>(client, LSP_MEM_CMD.ingest, { memory_session_id: memorySessionId, output });
  return result === undefined ? null : toMemoryIngestResult(result);
}

/**
 * Run one memory operation under a user-facing progress notification (the manual
 * command path: snapshot / diff / gc / references). The pause → evaluate → analyze
 * round-trip takes a beat, so its stages narrate under one notification
 * ([PROF-UI-STATE]).
 */
export async function runMemoryScript(
  store: Store,
  command: string,
  extraArgs: Record<string, unknown> = {},
): Promise<MemoryIngestResult | null> {
  return withUserProgress(
    memOpTitle(command),
    async (report) => runMemoryOperation({ store, command, extraArgs, report, quiet: false }),
  );
}

// ── Presentation ────────────────────────────────────────────────────────────

/**
 * Land a snapshot result: paint the allocation track, append a timeline
 * point, and retain it for a later "Compare". With `openResultsView` (the
 * manual "Take Memory Snapshot" affordance), open the memory dashboard
 * — the raw V8 `.heapprofile` stays one click away on its own button
 * ([PROF-CPU-START]). Without it (the autopilot's quiet per-pass capture) only
 * the decorations + timeline update — the diff step surfaces the dashboard.
 */
export function presentSnapshot(
  result: MemoryIngestResult,
  options: { openResultsView: boolean },
): void {
  applyMemoryDecorations(toSnapshotResult(result));
  const dashboard = toDashboardSnapshot(result);
  recordTimelinePoint(dashboard);
  dashboard.timeline = [...captureTimeline];
  lastDashboardSnapshot = dashboard;
  Logger.info(`Memory snapshot: ${dashboard.currentMemory} bytes current`);
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
  if (lastDashboardSnapshot !== undefined) {
    openMemoryDashboard(lastDashboardSnapshot, toDashboardDiff(result));
  }
  const leaks = diff.suspectedLeaks;
  Logger.info(`Memory diff: ${leaks.length} suspected leak(s)`);
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

/** Append a timeline point for a snapshot (seconds-epoch x-axis the chart reads). */
function recordTimelinePoint(snapshot: MemoryDashboardSnapshot): void {
  captureTimeline.push({
    timestamp: Date.now() / MS_PER_SECOND,
    currentMemory: snapshot.currentMemory,
    peakMemory: snapshot.peakMemory,
    gcObjects: snapshot.gcObjects,
  });
}

// ── At-exit final snapshot ──────────────────────────────────────────────────

/**
 * Read the debuggee's at-exit snapshot payload, deleting the file once it is read
 * intact ([PROF-PY-MEMORY-FINAL]). The debuggee writes atomically (sibling temp +
 * `os.replace`), so a readable marker-bearing file is whole. A short poll covers
 * the terminate-event/flush race. The file is unlinked ONLY once a complete
 * (marker-bearing) payload is read — a missing or marker-less read is never
 * destructive. Returns null when no usable payload arrived by the deadline.
 */
export async function readFinalSnapshot(path: string): Promise<string | null> {
  const deadline = Date.now() + FINAL_SNAPSHOT_WAIT_MS;
  for (;;) {
    const contents = await fs.promises.readFile(path, "utf8").catch(() => null);
    if (contents?.includes("__LSPPROF_MEM__") === true) {
      await fs.promises.unlink(path).catch(() => undefined);
      return contents;
    }
    if (Date.now() >= deadline) { return null; }
    await delay(FINAL_SNAPSHOT_POLL_MS);
  }
}

/** Read a `currentMemory` figure off an ingest result (for logging/summaries). */
export function snapshotCurrentMemory(result: MemoryIngestResult): number {
  return asNumber(result.currentMemory);
}
