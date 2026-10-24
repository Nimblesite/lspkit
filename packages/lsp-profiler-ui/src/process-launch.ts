// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Launch actions for the Processes panel.
 *
 * Two surfaces, one language: per-row inline actions ("Profile CPU" 🔥 /
 * "Track Memory" 🗄️ — the #62 fix, hardened for #79) and the metric-explicit
 * title-bar launches for the current file ("Run & Profile CPU" / "Run & Track
 * Memory" — the #82 fix). The tree rendering lives in process-explorer.ts;
 * this module owns what happens when the user clicks.
 */

import { numberField, rawField } from "./unknown-shape";
import { cmd, debugType, configSection, brand } from "./ids";
import * as vscode from "vscode";
import { startProfilingForPid } from "./profiler-attach";
import { type ProfilerStore as Store } from "./store";
import { type ProcessInfo } from "./process-explorer";
import type { LaunchMetric } from "./host";

// ── Per-row actions (#79) ────────────────────────────────────────────────

/** The slice of the panel's TreeView the launch actions read (test seam). */
export interface ProcessRowSource {
  readonly selection: readonly vscode.TreeItem[];
}

/** A tree row that targets a concrete process. */
type ProcessRowItem = vscode.TreeItem & { readonly process: ProcessInfo };

/** Structural guard: does this tree item carry a profilable process? */
function isProcessRow(item: vscode.TreeItem | undefined): item is ProcessRowItem {
  return numberField(rawField(item, "process"), "pid") !== undefined;
}

/**
 * Resolve the row an inline action targets. VS Code sometimes invokes inline
 * tree-view commands with no argument (#79 — e.g. a click racing the panel's
 * auto-refresh), so fall back to the panel's current selection before warning.
 */
function resolveProcessRow(
  item: vscode.TreeItem | undefined,
  view: ProcessRowSource,
): ProcessRowItem | undefined {
  if (isProcessRow(item)) { return item; }
  return view.selection.find((entry): entry is ProcessRowItem => isProcessRow(entry));
}

/** Guard: narrow to a real target, warning when an action has no row. */
function ensureSelection(item: ProcessRowItem | undefined): item is ProcessRowItem {
  if (item === undefined) {
    vscode.window.showWarningMessage(
      `${brand()}: Select a process row, then use Profile CPU or Track Memory.`,
    );
    return false;
  }
  return true;
}

/** Start CPU profiling the given process row — no input box (the #62 fix). */
async function profileProcess(store: Store, item: ProcessRowItem | undefined): Promise<void> {
  if (!ensureSelection(item)) { return; }
  // The blocked contextValue hides the menu actions, but a click can race the
  // panel's auto-refresh (or arrive via the palette) — never attempt an attach
  // the panel already knows will fail; explain why instead (#266).
  if (!item.process.debuggable) {
    vscode.window.showWarningMessage(
      `${brand()}: Can't profile PID ${String(item.process.pid)} — ${item.process.undebuggableReason ?? "the profiler cannot attach to it"}.`,
    );
    return;
  }
  const preset = vscode.workspace.getConfiguration(configSection()).get<string>("profiler.preset", "default");
  await startProfilingForPid(store, item.process.pid, preset);
}

/** Where a row's Track Memory action routes. */
export type MemoryTrackRoute = "start-tracking" | "offer-launch";

/** The slice of a debug session the memory route needs (test seam). */
type SessionRef = Pick<vscode.DebugSession, "id" | "type">;

/** Injectable environment for the row actions (test seam, like ProcessRowSource). */
export interface ProcessRowDeps {
  readonly runCommand: (command: string, ...args: unknown[]) => Thenable<unknown>;
  readonly activeSession: () => SessionRef | undefined;
}

const DEFAULT_ROW_DEPS: ProcessRowDeps = {
  runCommand: async (command, ...args) => await vscode.commands.executeCommand(command, ...args),
  activeSession: () => vscode.debug.activeDebugSession,
};

/**
 * Memory tracking rides the DAP-`evaluate` courier ([PROF-UI-MEMORY]),
 * so it can only ever target the live host debuggee — tracemalloc cannot
 * be injected into an arbitrary external PID. Anything else routes to the
 * "launch it under the debugger" offer instead of silently starting the wrong
 * (CPU) profiler.
 */
export function memoryTrackRoute(
  store: Store,
  pid: number,
  session: SessionRef | undefined,
): MemoryTrackRoute {
  const debuggeePid =
    session?.type === debugType() ? store.getDebuggeeProcessId(session.id) : undefined;
  return debuggeePid === pid ? "start-tracking" : "offer-launch";
}

/** Track memory for the given row: real tracking for the debuggee, an honest offer otherwise. */
async function memoryTrackProcess(
  store: Store,
  item: ProcessRowItem | undefined,
  deps: ProcessRowDeps,
): Promise<void> {
  if (!ensureSelection(item)) { return; }
  const pid = item.process.pid;
  // A direct runtime reads any attachable heap; a courier runtime only its debuggee ([PROF-UI-CAPABILITIES]).
  if (store.memoryIsDirect(item.process.runtime) || memoryTrackRoute(store, pid, deps.activeSession()) === "start-tracking") {
    await deps.runCommand(cmd("memoryStart"), pid);
    return;
  }
  offerMemoryLaunch(pid, deps);
}

/** A courier runtime tracks only its debuggee: offer the launch that makes `pid`'s program one. */
function offerMemoryLaunch(pid: number, deps: ProcessRowDeps): void {
  const launch = "Run & Track Memory (Current File)";
  const offer = `${brand()}: Memory tracking needs the target running under the ${brand()} debugger — PID ${String(pid)} is not the active debuggee. Launch the current file with memory tracking instead?`;
  void vscode.window.showWarningMessage(offer, launch).then((choice) => {
    if (choice === launch) { void deps.runCommand(cmd("trackMemoryCurrentFile")); }
  });
}

/**
 * The per-row launch handlers exactly as VS Code invokes them for the inline
 * buttons and context menu. Exported so tests can drive the real command
 * surface, including the no-argument invocation of issue #79.
 */
export function createProcessRowActions(
  store: Store,
  view: ProcessRowSource,
  deps: ProcessRowDeps = DEFAULT_ROW_DEPS,
): {
  readonly profileProcess: (item?: vscode.TreeItem) => Promise<void>;
  readonly memoryTrackProcess: (item?: vscode.TreeItem) => Promise<void>;
} {
  return {
    profileProcess: async (item) => { await profileProcess(store, resolveProcessRow(item, view)); },
    memoryTrackProcess: async (item) => { await memoryTrackProcess(store, resolveProcessRow(item, view), deps); },
  };
}

// ── Current-file launches (#82) ──────────────────────────────────────────
// Implements [PROF-UI-PROCESSES].
// See docs/specs/LSP-PROFILER-SPEC.md

/** Which metric a metric-explicit "Run & …" launch tracks (#82). */
export type { LaunchMetric } from "./host";

/**
 * Build the debug configuration for a metric-explicit run-and-profile launch.
 * Exported so tests can assert each metric produces the right launch shape.
 *
 * CPU sets `profileOnLaunch` (honoured by profiler.ts). Memory stops on entry
 * because tracemalloc is injected via DAP `evaluate`, which needs a paused
 * debuggee ([PROF-UI-MEMORY]); memory-profiler.ts starts tracking at the
 * entry pause and then resumes the program.
 */
async function profileCurrentFile(store: Store, metric: LaunchMetric): Promise<void> {
  const file = metricBusy(store, metric) ? undefined : profilableFile(store);
  if (file === undefined) { return; }
  const configuration = store.host.launchConfig(metric, file.fsPath);
  if (configuration === undefined) {
    vscode.window.showWarningMessage(`${brand()}: Running the current file for profiling is not supported here.`);
    return;
  }
  if (!(await vscode.debug.startDebugging(vscode.workspace.getWorkspaceFolder(file), configuration))) {
    vscode.window.showErrorMessage(`${brand()}: Could not launch the current file for profiling.`);
  }
}

/**
 * While the matching metric is starting or running the launch button is hidden
 * ([PROF-UI-PROCESSES]); the command is guarded per metric too, so the palette
 * path can't spawn a second SAME-metric session on top of the active one — yet a
 * CPU run while memory tracking is live (and vice versa) is fine.
 */
function metricBusy(store: Store, metric: LaunchMetric): boolean {
  const busy = metric === "cpu" ? store.cpuBusy.value : store.memoryBusy.value;
  if (busy) {
    const label = metric === "cpu" ? "CPU profile" : "memory tracking session";
    vscode.window.showWarningMessage(`${brand()}: A ${label} is already active. Stop it before running another.`);
  }
  return busy;
}

/** The active editor's file when it is in one of the host's languages; otherwise say what to open. */
function profilableFile(store: Store): vscode.Uri | undefined {
  const editor = vscode.window.activeTextEditor;
  if (editor === undefined || !store.host.languageIds.includes(editor.document.languageId)) {
    vscode.window.showWarningMessage(`${brand()}: Open a ${store.host.languageIds.join("/")} file to run and profile.`);
    return undefined;
  }
  return editor.document.uri;
}

// ── Registration ─────────────────────────────────────────────────────────

/** Register every launch command the Processes panel contributes. */
export function registerLaunchCommands(store: Store, view: ProcessRowSource): vscode.Disposable[] {
  const rowActions = createProcessRowActions(store, view);
  return [
    vscode.commands.registerCommand(cmd("profileProcess"), rowActions.profileProcess),
    vscode.commands.registerCommand(cmd("memoryTrackProcess"), rowActions.memoryTrackProcess),
    vscode.commands.registerCommand(cmd("profileCurrentFileCpu"), async () => { await profileCurrentFile(store, "cpu"); }),
    vscode.commands.registerCommand(cmd("trackMemoryCurrentFile"), async () => { await profileCurrentFile(store, "memory"); }),
  ];
}
