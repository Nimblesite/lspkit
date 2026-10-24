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

import * as vscode from "vscode";
import { cmd, brand } from "./ids";
import type { ProfilerStore as Store } from "./store";
import { bindMemoryStatusBar } from "./memory-status";
import { withUserProgress } from "./progress-ops";
import { disposeMemoryDashboard } from "./memory-dashboard";
import { disposeRefGraph } from "./memory-ref-graph";
import type { MemoryIngestResult } from "./memory-dashboard-mapping";
import { formatBytes } from "./profiler-styles";
import {
  clearMemoryDecorations,
  disposeMemoryDecorations,
} from "./memory-decorations";
import {
  LSP_MEM_CMD,
  memStartTitle,
  endServerSession,
  hasCapturedSnapshot,
  presentDiff,
  presentSnapshot,
  resetCaptureState,
  runMemoryScript,
} from "./memory-capture";
import {
  clearPendingSnapshotCleanup,
  handleMemoryStop,
  scheduleSnapshotCleanup,
  settleMemoryOnSessionEnd,
  showActionableToast,
} from "./memory-finalize";
import { handleMemoryStart, trackMemoryOnLaunch } from "./memory-start";
import { pickReferenceType, walkReferences } from "./memory-ref-picker";

// Re-exported so tests keep one import site for the memory profiler's seams.
export { memoryStatusText } from "./memory-status";

// ── State ─────────────────────────────────────────────────────────────────
//
// Memory-tracking session state lives in the store as a reactive signal
// ([PROF-UI-PROCESSES]); `boundStore` is the handle this module reads
// it through (and the e2e seam reads it through `activeMemorySession`).

let boundStore: Store | undefined;

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

/** The memory command registrations and debug-session listeners. */
function memoryCommandDisposables(store: Store): vscode.Disposable[] {
  return [
    vscode.commands.registerCommand(cmd("memoryMenu"), async () => { await handleMemoryMenu(store); }),
    vscode.commands.registerCommand(cmd("memoryStart"), async (pid?: unknown) => { await withUserProgress(memStartTitle(), async (report) => { await runMemoryStartCommand(store, report, typeof pid === "number" ? pid : undefined); },
      ); },
    ),
    vscode.commands.registerCommand(cmd("memorySnapshot"), async () => { await handleMemorySnapshot(store); }),
    vscode.commands.registerCommand(cmd("memoryDiff"), async () => { await handleMemoryDiff(store); }),
    vscode.commands.registerCommand(cmd("memoryGcCollect"), async () => { await handleMemoryGcCollect(store); }),
    vscode.commands.registerCommand(cmd("memoryStop"), async () => { await runMemoryStopCommand(store); }),
    vscode.commands.registerCommand(cmd("memoryReferences"), async () => { await handleMemoryReferences(store); }),
    // The status-bar show/hide on session changes is bound in memory-status.ts.
    vscode.debug.onDidStartDebugSession((session) => { trackMemoryOnLaunch(store, session); }),
    vscode.debug.onDidTerminateDebugSession((session) => { settleMemoryOnSessionEnd(store, session); }),
  ];
}

/** The `<prefix>.profiler.memoryStart` body: start tracking, then narrate what comes next. */
async function runMemoryStartCommand(store: Store, report: (message: string) => void, pid?: number): Promise<void> {
  if (await handleMemoryStart(store, report, pid)) {
    // With the autopilot on ([PROF-UI-AUTOPILOT]), the user just sets
    // breakpoints and presses Continue — each pause is captured automatically;
    // and even a breakpoint-free run captures a final snapshot at exit
    // ([PROF-PY-MEMORY-FINAL]) — so this is never a dead end. The moment of
    // disorientation is right here (the user just landed in the Debug view),
    // so the toast carries the action ([PROF-UI-MEMORY], #263).
    showActionableToast(
      `${brand()}: Memory tracking started. Press Continue to auto-capture each pause, or let the program finish for an automatic final snapshot.`,
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
async function runMemoryStopCommand(store: Store): Promise<void> {
  const hadSnapshot = hasCapturedSnapshot();
  scheduleSnapshotCleanup(store);
  await endServerSession(store);
  handleMemoryStop(store);
  if (hadSnapshot) {
    void vscode.window.showInformationMessage(`${brand()}: Memory tracking stopped.`);
    return;
  }
  // The toast tells the user to take a snapshot, so it offers the way there
  // (the quick-pick action menu), never an invisible palette ([PROF-UI-MEMORY], #263).
  showActionableToast(
    `${brand()}: Memory tracking stopped — no snapshot was taken. Take a snapshot while paused, or let the program finish, to inspect allocations.`,
    "Memory Actions…",
    cmd("memoryMenu"),
  );
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

/** Quick-pick menu of memory actions — the clickable alternative to the palette. */
async function handleMemoryMenu(store: Store): Promise<void> {
  const tracking = store.profiler.value.memory === "active";
  const items: { label: string; command: string }[] = tracking
    ? [
        { label: "$(device-camera) Take Memory Snapshot", command: cmd("memorySnapshot") },
        { label: "$(diff) Compare Memory Snapshots", command: cmd("memoryDiff") },
        { label: "$(type-hierarchy) Show Reference Graph", command: cmd("memoryReferences") },
        { label: "$(trash) Force Garbage Collection", command: cmd("memoryGcCollect") },
        { label: "$(debug-stop) Stop Memory Tracking", command: cmd("memoryStop") },
      ]
    : [{ label: "$(database) Start Memory Tracking", command: cmd("memoryStart") }];
  const pick = await vscode.window.showQuickPick(items, {
    placeHolder: tracking ? `${brand()} memory profiling` : "Start memory tracking (briefly pauses the program)",
  });
  if (pick !== undefined) {
    await vscode.commands.executeCommand(pick.command);
  }
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

// ── Command handlers ──────────────────────────────────────────────────────

async function handleMemorySnapshot(store: Store): Promise<void> {
  const result = await runMemoryScript(store, LSP_MEM_CMD.snapshot);
  if (result?.kind === "snapshot") {
    presentSnapshot(result, { openResultsView: true });
  }
}

async function handleMemoryDiff(store: Store): Promise<void> {
  const result = await runMemoryScript(store, LSP_MEM_CMD.diff);
  if (result?.kind === "diff") {
    const diff = presentDiff(result);
    void vscode.window.showInformationMessage(
      `${brand()}: Compared snapshots — ${String(diff.suspectedLeaks.length)} suspected leak(s)`,
    );
  }
}

async function handleMemoryGcCollect(store: Store): Promise<void> {
  const result = await runMemoryScript(store, LSP_MEM_CMD.gcCollect);
  if (result?.kind === "gc") {
    void vscode.window.showInformationMessage(gcSummary(result));
  }
}

/**
 * The toast after a forced collection, in terms every runtime shares: bytes
 * released and, when the runtime reports any, what could not be collected.
 * `collected` is runtime-defined (objects for some, collections observed for
 * others — [PROF-MEMORY-OUTCOME]), so it is never shown as an object count.
 */
export function gcSummary(result: MemoryIngestResult): string {
  const freed = formatBytes(Math.max(0, Number(result.freedBytes ?? 0)));
  const uncollectable = Number(result.uncollectable ?? 0);
  const stuck = uncollectable > 0 ? `; ${String(uncollectable)} object(s) could not be collected` : "";
  return `${brand()}: Garbage collection ran — freed ${freed}${stuck}`;
}

/**
 * Show Reference Graph — pick a type from the data-driven Quick Pick (the active
 * file's classes + container builtins) and walk its retainers
 * ([PROF-UI-REFGRAPH]); no blank "type a name" box.
 */
async function handleMemoryReferences(store: Store): Promise<void> {
  if (store.profiler.value.memory !== "active") {
    void vscode.window.showWarningMessage(`${brand()}: Start memory tracking first.`);
    return;
  }
  const typeName = await pickReferenceType();
  if (typeName === undefined || typeName.trim() === "") { return; }
  await walkReferences(store, typeName.trim());
}

