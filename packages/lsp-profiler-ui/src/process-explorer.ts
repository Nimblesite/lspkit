// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Processes — TreeDataProvider for the host's sidebar.
 *
 * The headline fix for #62: instead of a raw PID input box, the user picks a
 * running Python process from this panel and starts CPU/memory profiling with
 * one click. The process list is owned by the LSP (`<prefix>.lspProfiler.processes`,
 * implemented over `sysinfo`) and fetched into the store's `processes` Signal by
 * the store-side poll (process-poll.ts). This provider is a pure projection of
 * that centralised state (#148): it holds no data or timer of its own — it
 * sorts, groups, filters, and renders whatever the store says, re-rendering on
 * each `processesRevision` bump exactly like the Modules panel
 * ([PROF-UI-STATE]).
 */

import * as vscode from "vscode";
import { cmd } from "./ids";
import type { ProfilerStore as Store } from "./store";
import { fetchProcessesIntoStore } from "./process-poll";
import type { ProcessPanelState } from "./processes-state";
import {
  GROUP_LABEL,
  LaunchActionItem,
  MessageTreeItem,
  ProcessGroupItem,
  ProcessTreeItem,
  SORT_COMPARATORS,
  SORT_LABEL,
} from "./process-explorer-rows";
import type { GroupMode, ProcessInfo, TreeItem } from "./process-explorer-rows";

// The model and row presentation live in process-explorer-rows.ts (500-LOC
// split), the reactive state in processes-state.ts (#148); re-exported here so
// consumers keep one import site for the panel.
export { type ProcessInfo } from "./process-explorer-rows";
export { type ProcessesFetchState as ProcessesState } from "./processes-state";
export { ProcessDecorationProvider } from "./process-decoration";

/** How long the "sorted/grouped by …" status hint stays visible (ms). */
const STATUS_HINT_MS = 2000;

// ── Provider ─────────────────────────────────────────────────────────────

export class ProcessesProvider implements vscode.TreeDataProvider<TreeItem>, vscode.Disposable {
  private readonly emitter = new vscode.EventEmitter<TreeItem | undefined>();
  public readonly onDidChangeTreeData = this.emitter.event;

  public readonly disposables: vscode.Disposable[] = [];

  private readonly store: Store;
  public constructor(store: Store) {
    this.store = store;
  }

  /** Repaint from the store's current state (no fetch — pure projection, #148). */
  public refresh(): void {
    this.emitter.fire(undefined);
  }

  /**
   * The current process-fetch state — the e2e seam for [PROF-UI-PROCESSES]
   * empty-state honesty (#147), mirrored to the `<prefix>.profiler.processesState` key.
   */
  public get processesState(): ProcessPanelState["fetch"] {
    return this.store.processes.value.fetch;
  }

  /**
   * Fetch fresh process data into the store — awaitable, so the manual Refresh
   * command can run it under the view's progress bar ([PROF-UI-STATE]).
   * The resulting revision bump repaints every subscribed view; the silent
   * store-side poll needs no progress bar (a flash every tick would be noise).
   */
  public async refreshNow(): Promise<void> {
    await fetchProcessesIntoStore(this.store);
  }

  public cycleSortMode(): void {
    const mode = this.store.cycleProcessSort();
    void vscode.window.setStatusBarMessage(`Processes sorted by ${SORT_LABEL[mode]}`, STATUS_HINT_MS);
  }

  public cycleGroupMode(): void {
    const mode = this.store.cycleProcessGroup();
    void vscode.window.setStatusBarMessage(`Processes grouped by ${GROUP_LABEL[mode]}`, STATUS_HINT_MS);
  }

  public setFilter(text: string): void {
    this.store.setProcessFilter(text);
  }

  public dispose(): void {
    for (const disposable of this.disposables) { disposable.dispose(); }
    this.emitter.dispose();
  }

  public getTreeItem(element: TreeItem): vscode.TreeItem {
    return element;
  }

  public getChildren(element?: TreeItem): TreeItem[] {
    const state = this.store.processes.value;
    if (element instanceof ProcessGroupItem) {
      // Re-sort so non-debuggable rows sink to the bottom *within the group* too,
      // not just in the flat list ([PROF-UI-PROCESSES]).
      return sortProcesses([...element.members], state).map((proc) => this.processRow(proc));
    }
    // Every other row is a leaf. With no processes at all, [] lets the
    // empty/loading/error `viewsWelcome` (which carries the big launch buttons)
    // render honestly.
    if (element !== undefined || state.list.length === 0) {
      return [];
    }
    return this.topLevelRows(state);
  }

  /**
   * Processes exist, so the welcome can't show: the current-file launches are
   * pinned to the top as rows ([PROF-UI-PROCESSES]), then the processes.
   */
  private topLevelRows(state: ProcessPanelState): TreeItem[] {
    const actions = this.launchActionRows();
    const visible = sortProcesses(applyFilter(state), state);
    if (visible.length === 0) {
      // The user's search filter hid every running process — keep the launches
      // and an honest placeholder rather than an empty list (procexp-2).
      return [...actions, new MessageTreeItem(filteredEmptyLabel(state))];
    }
    const rows = state.groupMode === "none"
      ? visible.map((proc) => this.processRow(proc))
      : buildGroups(visible, state.groupMode);
    return [...actions, ...rows];
  }

  /**
   * Render one process row. The "this row is being profiled" marker derives
   * straight from the store's profiler signal — never a provider field (#148) —
   * and the active-debuggee marker from the centralised panel state.
   */
  private processRow(proc: ProcessInfo): ProcessTreeItem {
    const session = this.store.profiler.value;
    const profilingPid = session.cpu === "active" ? session.cpuPid : undefined;
    const debuggee = this.store.processes.value.activeDebuggeePid;
    return new ProcessTreeItem(proc, {
      activeProfilingPid: profilingPid,
      activeDebuggeePid: debuggee,
      runtime: {
        directMemory: this.store.memoryIsDirect(proc.runtime),
        name: this.store.runtimeName(proc.runtime),
        sourceEntries: this.store.sourceEntries(proc.runtime),
        memory: this.store.tracksMemory(proc.runtime),
      },
    });
  }

  /**
   * The pinned current-file launch rows, gated per-activity: the CPU launch is
   * hidden while CPU profiling is busy, the memory launch while memory tracking
   * is busy — so a second same-metric run is never offered, yet either can start
   * while the other runs ([PROF-UI-PROCESSES]). A server none of whose runtimes
   * tracks memory never gets the memory launch ([PROF-UI-CAPABILITIES]).
   */
  private launchActionRows(): LaunchActionItem[] {
    const rows: LaunchActionItem[] = [];
    if (!this.store.cpuBusy.value) {
      rows.push(new LaunchActionItem("Run & Profile CPU (Current File)", cmd("profileCurrentFileCpu"), "flame"));
    }
    if (!this.store.memoryBusy.value && this.store.memoryOffered()) {
      rows.push(new LaunchActionItem("Run & Track Memory (Current File)", cmd("trackMemoryCurrentFile"), "database"));
    }
    return rows;
  }
}

// ── Pure projection helpers (#148) ─────────────────────────────────────────

/**
 * Sort by the active mode, but always sink non-`debuggable` rows to the bottom
 * so the processes the user can act on stay on top ([PROF-UI-PROCESSES]).
 */
function sortProcesses(processes: ProcessInfo[], state: ProcessPanelState): ProcessInfo[] {
  const byMode = SORT_COMPARATORS[state.sortMode];
  return processes.toSorted((left, right) => {
    if (left.debuggable !== right.debuggable) { return left.debuggable ? -1 : 1; }
    return byMode(left, right);
  });
}

/** Why the filtered view is empty though processes are running (procexp-2). */
function filteredEmptyLabel(state: ProcessPanelState): string {
  return `No process matches "${state.filterText}" (${String(state.list.length)} running)`;
}

/**
 * Apply only the user's explicit search filter. Enumeration is zero-filter
 * ([PROF-PROCESSES-SCOPE]); the panel never auto-hides a process.
 */
function applyFilter(state: ProcessPanelState): ProcessInfo[] {
  if (state.filterText === "") { return [...state.list]; }
  return state.list.filter((proc) => {
    const haystack = `${proc.name} ${proc.script ?? ""} ${String(proc.pid)}`.toLowerCase();
    return haystack.includes(state.filterText);
  });
}

function buildGroups(processes: readonly ProcessInfo[], groupMode: GroupMode): ProcessGroupItem[] {
  const groups = new Map<string, ProcessInfo[]>();
  for (const proc of processes) {
    const key = groupKey(proc, groupMode);
    const bucket = groups.get(key);
    if (bucket === undefined) {
      groups.set(key, [proc]);
    } else {
      bucket.push(proc);
    }
  }
  return [...groups.entries()]
    .toSorted((left, right) => left[0].localeCompare(right[0]))
    .map(([key, members]) => new ProcessGroupItem(key, members));
}

function groupKey(proc: ProcessInfo, groupMode: GroupMode): string {
  if (groupMode === "version") { return proc.runtimeVersion ?? "Unknown version"; }
  if (groupMode === "interpreter") { return proc.interpreterPath ?? proc.name; }
  if (groupMode === "user") { return proc.user ?? "Unknown user"; }
  if (groupMode === "parent") { return `Parent ${String(proc.ppid)}`; }
  return "";
}
