// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Memory profiler command + lifecycle layer for the lsp-profiler UI library.
 *
 * Owns the `<prefix>.profiler.memory*` command registrations, the memory status-bar
 * indicator, the track-on-launch auto-start, and the at-exit finalisation
 * ([PROF-PY-MEMORY-FINAL]). The editor-as-courier round-trip and result
 * presentation live in [`memory-capture.ts`](./memory-capture.ts); the
 * reference-graph type picker in [`memory-ref-picker.ts`](./memory-ref-picker.ts);
 * the automatic per-pause / interval capture in
 * [`memory-autopilot.ts`](./memory-autopilot.ts). All memory analysis logic lives
 * in the LSP — this module is client-side UI and command routing.
 */

import * as vscode from "vscode";
import { cmd, debugType, brand } from "./ids";
import { Logger } from "./logger";
import type { ProfilerStore as Store } from "./store";
import { bindMemoryStatusBar } from "./memory-status";
import { acquireStoppedFrame, evaluateInDebugSession, waitForStoppedFrame } from "./dap-evaluate";
import { withUserProgress } from "./progress-ops";
import { toMemoryIngestResult } from "./wire-adapters";
import type { MemoryResult, MemoryStartResult } from "./wire-generated";
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
  TRACEBACK_DEPTH,
  hasCapturedSnapshot,
  presentDiff,
  presentSnapshot,
  readFinalSnapshot,
  resetCaptureState,
  runMemoryScript,
} from "./memory-capture";
import { pickReferenceType, walkReferences } from "./memory-ref-picker";

// Re-exported so tests keep one import site for the memory profiler's seams.
export { memoryStatusText } from "./memory-status";

// ── State ─────────────────────────────────────────────────────────────────
//
// Memory-tracking session state lives in the store as a reactive signal
// ([PROF-UI-PROCESSES]); `boundStore` is the handle this module reads
// it through (and the e2e seam reads it through `activeMemorySession`).

let boundStore: Store | undefined;
/**
 * Final-snapshot files awaiting cleanup, keyed by debug-session id. When the
 * user stops tracking *mid-run*, the debuggee's `atexit` hook is still armed and
 * will write its file when the program eventually exits; we delete it on that
 * session's termination so a manual stop never orphans a temp file
 * ([PROF-PY-MEMORY-FINAL]).
 */
const pendingSnapshotCleanup = new Map<string, string>();

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
    vscode.commands.registerCommand(cmd("memoryMenu"), async () => handleMemoryMenu(store)),
    vscode.commands.registerCommand(cmd("memoryStart"), async (pid?: unknown) =>
      withUserProgress(memStartTitle(), async (report) =>
        runMemoryStartCommand(store, report, typeof pid === "number" ? pid : undefined),
      ),
    ),
    vscode.commands.registerCommand(cmd("memorySnapshot"), async () => handleMemorySnapshot(store)),
    vscode.commands.registerCommand(cmd("memoryDiff"), async () => handleMemoryDiff(store)),
    vscode.commands.registerCommand(cmd("memoryGcCollect"), async () => handleMemoryGcCollect(store)),
    vscode.commands.registerCommand(cmd("memoryStop"), () => { runMemoryStopCommand(store); }),
    vscode.commands.registerCommand(cmd("memoryReferences"), async () => handleMemoryReferences(store)),
    // The status-bar show/hide on session changes is bound in memory-status.ts.
    vscode.debug.onDidStartDebugSession((session) => {
      // "Run & Track Memory (Current File)" (#82): the launch stopped on
      // entry; inject tracemalloc there, then resume the program.
      if (session.type === debugType() && session.configuration.memoryTrackOnLaunch === true) {
        void startMemoryTrackingOnLaunch(store);
      }
    }),
    // Only the *tracked* session's termination finalises the run into a visible
    // result from the at-exit snapshot ([PROF-PY-MEMORY-FINAL], #146) — an
    // unrelated debug session ending must never tear down live tracking. A
    // manually-stopped session deletes its now-orphaned at-exit file here.
    vscode.debug.onDidTerminateDebugSession((session) => {
      // Finalise when the tracked session ends. If we never captured a concrete
      // session id — `activeDebugSession` was momentarily undefined at start
      // (memui-3) — fall back to ANY host-debug-type session ending while tracking
      // is active: only one memory session exists at a time, so it is the tracked
      // one. This guarantees tracking always settles instead of hanging on
      // "Memory: tracking" forever (no stale state on screen).
      const trackedId = store.profiler.value.memoryDebugSessionId;
      const tracking = store.profiler.value.memory !== "idle";
      const isTrackedSession =
        trackedId !== undefined ? session.id === trackedId : session.type === debugType();
      if (tracking && isTrackedSession) {
        pendingSnapshotCleanup.delete(session.id);
        void finalizeMemorySessionOnEnd(store);
        return;
      }
      const orphan = pendingSnapshotCleanup.get(session.id);
      if (orphan !== undefined) {
        pendingSnapshotCleanup.delete(session.id);
        void readFinalSnapshot(orphan); // reads + unlinks; the payload is discarded
      }
    }),
  ];
}

/**
 * Show a toast whose named action is offered as a button — a message must
 * never demand an action it doesn't offer ([PROF-UI-MEMORY], #263).
 * Fire-and-forget: a sticky notification must not block the flow.
 */
function showActionableToast(message: string, action: string, command: string): void {
  void vscode.window.showInformationMessage(message, action).then((choice) => {
    if (choice === action) {
      void vscode.commands.executeCommand(command);
    }
  });
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

/** The `<prefix>.profiler.memoryStop` body: stop tracking and never report nothing. */
function runMemoryStopCommand(store: Store): void {
  // Stopping must never silently produce nothing: surface whether a snapshot was
  // captured, or say plainly that none was ([PROF-PY-MEMORY-FINAL], #146).
  const hadSnapshot = hasCapturedSnapshot();
  // Stopping mid-run leaves the debuggee's `atexit` hook armed; schedule the
  // file it will write for cleanup when that session terminates, so a manual
  // stop never orphans a temp file.
  const debugSessionId = store.profiler.value.memoryDebugSessionId;
  const finalSnapshotFile = store.profiler.value.memoryFinalSnapshotFile;
  if (debugSessionId !== undefined && finalSnapshotFile !== undefined) {
    pendingSnapshotCleanup.set(debugSessionId, finalSnapshotFile);
  }
  handleMemoryStop(store);
  if (hadSnapshot) {
    void vscode.window.showInformationMessage(`${brand()}: Memory tracking stopped.`);
    return;
  }
  // The toast tells the user to take a snapshot — it must offer the way there
  // (the quick-pick action menu), never point at an invisible palette
  // ([PROF-UI-MEMORY], #263).
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
  pendingSnapshotCleanup.clear();
}

// ── Track-memory-on-launch ────────────────────────────────────────────────

/**
 * Auto-start flow for the "Run & Track Memory (Current File)" entry point
 * (#82). Memory tracking needs a paused debuggee ([PROF-UI-MEMORY]), so
 * the launch config sets `stopOnEntry`; tracemalloc is injected at the entry
 * pause and the debuggee is resumed so the program runs with tracking on. With a
 * breakpoint set, the autopilot then captures each pass automatically
 * ([PROF-UI-AUTOPILOT]); with none, a final snapshot opens at exit
 * ([PROF-PY-MEMORY-FINAL]). Implements [PROF-UI-PROCESSES] (memory leg).
 */
async function startMemoryTrackingOnLaunch(store: Store): Promise<void> {
  // One notification spans the whole auto-start ([PROF-UI-STATE]).
  await withUserProgress(memStartTitle(), async (report) => {
    report("Waiting for the program to pause at entry…");
    const frameId = await waitForStoppedFrame();
    if (frameId === null) {
      void vscode.window.showWarningMessage(
        `${brand()}: The debuggee did not pause at entry — pause at a breakpoint, then start memory tracking.`,
      );
      return;
    }
    if (await handleMemoryStart(store, report)) {
      Logger.info("Memory tracking started on launch — resuming the debuggee");
      // State the outcome up front so the toast never points nowhere: a run with
      // a breakpoint auto-captures each pass ([PROF-UI-AUTOPILOT]);
      // one without finalises at exit ([PROF-PY-MEMORY-FINAL], #146).
      const hasBreakpoints = vscode.debug.breakpoints.length > 0;
      void vscode.window.showInformationMessage(
        hasBreakpoints
          ? `${brand()}: Tracking memory — press Continue and each pause is captured automatically.`
          : `${brand()}: Tracking memory — a final snapshot opens automatically when the program finishes.`,
      );
      report("Resuming the program…");
      await vscode.commands.executeCommand("workbench.action.debug.continue");
    }
  });
}

// ── Command handlers ──────────────────────────────────────────────────────

/**
 * Inject tracemalloc into the paused debuggee and adopt the session. Returns
 * whether tracking actually started, so each caller can show its own
 * context-appropriate message (the menu vs. the run-and-track-on-launch flow)
 * instead of a one-size-fits-all toast. The leg-1 response carries the at-exit
 * `finalSnapshotFile` the session is finalised from on session end
 * ([PROF-PY-MEMORY-FINAL]).
 */
async function handleMemoryStart(
  store: Store,
  report: (message: string) => void,
  pid?: number,
): Promise<boolean> {
  const client = store.client.value;
  if (client?.isRunning() !== true) {
    void vscode.window.showErrorMessage(`${brand()} LSP not connected`);
    return false;
  }
  // Memory starts gate on the memory leg only ([PROF-UI-PROCESSES]) —
  // independent of any live CPU profile, but never a second memory session.
  if (store.memoryBusy.value) {
    void vscode.window.showWarningMessage(`${brand()}: Memory tracking is already active.`);
    return false;
  }
  // The target: an explicit PID (a processes-panel row) or the active debuggee.
  const debugSession = vscode.debug.activeDebugSession;
  const target = pid ?? (debugSession === undefined ? undefined : store.getDebuggeeProcessId(debugSession.id));
  if (target === undefined) {
    void vscode.window.showWarningMessage(`${brand()}: Pick a process in the panel, or debug a program, to track its memory.`);
    return false;
  }
  // From here on the panel reflects a tracking start in flight; every exit path
  // must settle it back to active or idle ([PROF-UI-STATE]).
  store.memoryTrackingStarting();
  try {
    report("Starting the memory session…");
    const result = await client.sendRequest<MemoryStartResult | null>("workspace/executeCommand", {
      command: LSP_MEM_CMD.start,
      arguments: [{ pid: target, traceback_depth: TRACEBACK_DEPTH }],
    });
    if (result === null || result === undefined) {
      store.memoryTrackingStopped();
      return false;
    }
    if (result.step.kind === "Evaluate" && !(await injectCourier(result.step.script, report))) {
      store.memoryTrackingStopped();
      return false;
    }
    // Remember which debug session this tracks, so only its termination
    // finalises the run ([PROF-PY-MEMORY-FINAL]).
    const tracked = store.getDebuggeeProcessId(debugSession?.id ?? "") === target ? debugSession?.id : undefined;
    store.memoryTrackingActive(result.memory_session_id, undefined, tracked);
    Logger.info(`Memory tracking started: session ${result.memory_session_id} (${result.mode.kind})`);
    return true;
  } catch (err) {
    store.memoryTrackingStopped();
    void vscode.window.showErrorMessage(
      `Memory tracking failed: ${err instanceof Error ? err.message : String(err)}`,
    );
    return false;
  }
}

/**
 * Courier runtimes ([PROF-MEMORY-MODE]) start tracking by running a script in
 * the paused debuggee: pause transparently, inject, resume.
 */
async function injectCourier(script: string, report: (message: string) => void): Promise<boolean> {
  report("Pausing the program…");
  const acquired = await acquireStoppedFrame();
  if (acquired === null) {
    void vscode.window.showWarningMessage(
      `${brand()}: Could not pause the program — pause at a breakpoint, then start memory tracking.`,
    );
    return false;
  }
  try {
    report("Injecting the tracker…");
    const ack = await evaluateInDebugSession(script, acquired.frameId);
    if (ack === null) {
      void vscode.window.showWarningMessage(`${brand()}: Could not start the memory tracker in the debuggee.`);
      return false;
    }
    return true;
  } finally {
    await acquired.release();
  }
}

async function handleMemorySnapshot(store: Store): Promise<void> {
  const result = await runMemoryScript(store, LSP_MEM_CMD.snapshot);
  if (result?.kind === "snapshot") {
    presentSnapshot(result, { openResultsView: true });
  }
}

/**
 * Finalise a memory-tracking session when its debug session ends — the fix for
 * the "Run & Track Memory" dead-end ([PROF-PY-MEMORY-FINAL], #146). A run with
 * no breakpoint completes with no paused frame to snapshot from, so the start
 * script registered an `atexit` hook that wrote a final snapshot to a file as
 * the program exited. Tear down the (now-stale) tracking state first so the
 * panel settles immediately, then read that file, ingest it through the normal
 * `<prefix>.lspProfiler.memory.ingest` path, and present the snapshot exactly like a manual
 * one. If nothing was captured (a crash, `os._exit`, or no allocations), say so
 * explicitly — the session end never silently produces nothing.
 */
async function finalizeMemorySessionOnEnd(store: Store): Promise<void> {
  const memorySessionId = store.profiler.value.memorySessionId;
  const finalSnapshotFile = store.profiler.value.memoryFinalSnapshotFile;
  handleMemoryStop(store);

  if (memorySessionId === undefined || finalSnapshotFile === undefined) { return; }
  const output = await readFinalSnapshot(finalSnapshotFile);
  if (output === null) {
    // No file / no marker: either the program made no tracked allocations, or it
    // exited too abruptly for even the SIGTERM/SIGINT hook to run (a native
    // crash, SIGKILL, or os._exit). Don't claim it "finished" — that is wrong
    // for a kill (ux-1).
    void vscode.window.showInformationMessage(
      `${brand()}: No final memory snapshot was captured — the program either made no tracked allocations or exited abruptly (a crash, kill, or os._exit). Set a breakpoint and take a snapshot to inspect allocations.`,
    );
    return;
  }

  // The snapshot WAS captured; from here every failure to turn it into a view is
  // surfaced, never swallowed — the spec's "stopping never silently produces
  // nothing" guarantee covers the post-capture paths too (memui-2).
  const captured = `${brand()}: Captured a final memory snapshot at exit, but `;
  const client = store.client.value;
  if (client?.isRunning() !== true) {
    void vscode.window.showWarningMessage(`${captured}the language server is not running, so it could not be analyzed.`);
    return;
  }
  try {
    const raw = await client.sendRequest<MemoryResult | null>("workspace/executeCommand", {
      command: LSP_MEM_CMD.ingest,
      arguments: [{ memory_session_id: memorySessionId, output }],
    });
    const result = raw === null || raw === undefined ? null : toMemoryIngestResult(raw);
    if (result?.kind === "snapshot") {
      presentSnapshot(result, { openResultsView: true });
      void vscode.window.showInformationMessage(
        `${brand()}: Captured a final memory snapshot at exit — opened the allocation view.`,
      );
      return;
    }
    void vscode.window.showWarningMessage(`${captured}it could not be analyzed into an allocation view.`);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    Logger.warn(`[Memory] final snapshot ingest failed: ${msg}`);
    void vscode.window.showWarningMessage(`${captured}analyzing it failed — ${msg}.`);
  }
}

async function handleMemoryDiff(store: Store): Promise<void> {
  const result = await runMemoryScript(store, LSP_MEM_CMD.diff);
  if (result?.kind === "diff") {
    const diff = presentDiff(result);
    void vscode.window.showInformationMessage(
      `${brand()}: Compared snapshots — ${diff.suspectedLeaks.length} suspected leak(s)`,
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
  const stuck = uncollectable > 0 ? `; ${uncollectable} object(s) could not be collected` : "";
  return `${brand()}: Garbage collection ran — freed ${freed}${stuck}`;
}

function handleMemoryStop(store: Store): void {
  store.memoryTrackingStopped();
  resetCaptureState();
  clearMemoryDecorations();
  Logger.info("Memory tracking stopped");
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

