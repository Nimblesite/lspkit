// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The centralised, reactive state of the Processes panel (#148).
 *
 * The panel used to own its data privately — `this.processes`, `this.fetched`,
 * sort/group/filter fields — and refresh itself on a panel-local timer. Per the
 * architecture (CLAUDE.md: all mutable state lives in the single store as
 * Signals; no panel polls itself), that state is hoisted here: the store owns
 * one `ProcessPanelState` Signal, the store-side poll (process-poll.ts) feeds
 * it, and the tree provider renders it as a pure projection, subscribing via
 * `subscribeRevision` exactly like the Modules panel.
 *
 * Defined outside store.ts so the store file only grows by the signal wiring,
 * not the type surface and action bodies (the profiler-state.ts pattern).
 */

import type { Signal } from "@preact/signals-core";
import {
  GROUP_CYCLE,
  SORT_CYCLE,
  type GroupMode,
  type ProcessInfo,
  type SortMode,
} from "./process-explorer-rows";

/**
 * The process-fetch lifecycle, mirrored to the `<prefix>.profiler.processesState` context
 * key so the empty-state welcome never lies: "No processes running" shows
 * only after a fetch actually succeeded (`loaded`), while a still-loading or
 * errored fetch says so honestly ([PROF-UI-PROCESSES], #147).
 */
export type ProcessesFetchState = "loading" | "loaded" | "error";

/**
 * Reactive snapshot of everything the Processes panel renders from.
 * Copy-on-write: actions replace the whole object (bumping `revision`) so the
 * backing Signal notifies subscribers.
 */
export interface ProcessPanelState {
  /** The last process table fetched from the LSP (empty until a fetch lands). */
  readonly list: readonly ProcessInfo[];
  /** Fetch lifecycle behind the welcome's empty/loading/error honesty (#147). */
  readonly fetch: ProcessesFetchState;
  /** Active sort mode (cycled from the title bar). */
  readonly sortMode: SortMode;
  /** Active grouping mode (cycled from the title bar). */
  readonly groupMode: GroupMode;
  /** The user's search filter, pre-normalised (trimmed, lowercased). */
  readonly filterText: string;
  /** PID of the active host debuggee — the only row that can be memory-tracked. */
  readonly activeDebuggeePid: number | undefined;
  /**
   * Monotonic change counter, bumped by every action. Panels subscribe to it via
   * `subscribeRevision` ([PROF-UI-STATE]) and re-render on each bump.
   */
  readonly revision: number;
}

/**
 * The initial state: nothing fetched yet, so the welcome honestly says "loading".
 * Rows open grouped by runtime version ([PROF-UI-PROCESSES]).
 */
export const IDLE_PROCESS_PANEL: ProcessPanelState = {
  list: [],
  fetch: "loading",
  sortMode: "cpu",
  groupMode: "version",
  filterText: "",
  activeDebuggeePid: undefined,
  revision: 0,
};

/** The only way to mutate the process-panel state — exposed on the Store. */
export interface ProcessPanelActions {
  /**
   * A fetch could not run (LSP not running yet). Stays honestly "loading" —
   * never asserting the definitive "no processes" (#147) — and clears any rows
   * from a previous server session so the tree never shows stale processes.
   */
  processesLoading(): void;
  /** A fetch succeeded; only now is an empty list a genuine "no processes" (#147). */
  processesLoaded(list: readonly ProcessInfo[]): void;
  /** A fetch failed — the welcome says "couldn't load", not "no processes" (#147). */
  processesFetchFailed(): void;
  /** Advance to the next sort mode and return it (for the status-bar hint). */
  cycleProcessSort(): SortMode;
  /** Advance to the next grouping mode and return it (for the status-bar hint). */
  cycleProcessGroup(): GroupMode;
  /** Set the search filter (normalised here so every reader sees one form). */
  setProcessFilter(text: string): void;
  /** Mark which PID is the active host debuggee; `undefined` clears it. */
  setActiveDebuggeePid(pid: number | undefined): void;
}

/** The mode after `current` in `cycle`, wrapping round. */
function nextInCycle<T>(cycle: readonly T[], current: T): T {
  return cycle[(cycle.indexOf(current) + 1) % cycle.length] ?? current;
}

/** Apply `next` to the panel state, bumping its revision so every subscribed panel repaints (#148). */
function patchPanel(panel: Signal<ProcessPanelState>, next: Partial<ProcessPanelState>): void {
  panel.value = { ...panel.value, ...next, revision: panel.value.revision + 1 };
}

type Patch = (next: Partial<ProcessPanelState>) => void;

/** Build the process-panel actions over the store's backing Signal. */
export function createProcessPanelActions(panel: Signal<ProcessPanelState>): ProcessPanelActions {
  const patch: Patch = (next) => { patchPanel(panel, next); };
  return { ...fetchActions(patch), ...viewActions(panel, patch) };
}

/** What each fetch outcome leaves in the panel (#147). */
function fetchActions(patch: Patch): Pick<ProcessPanelActions, "processesLoading" | "processesLoaded" | "processesFetchFailed"> {
  return {
    processesLoading() { patch({ list: [], fetch: "loading" }); },
    processesLoaded(list) { patch({ list, fetch: "loaded" }); },
    processesFetchFailed() { patch({ list: [], fetch: "error" }); },
  };
}

/** How the user arranges the rows, and which row is the debuggee. */
function viewActions(panel: Signal<ProcessPanelState>, patch: Patch): Omit<ProcessPanelActions, keyof ReturnType<typeof fetchActions>> {
  return {
    cycleProcessSort() {
      const sortMode = nextInCycle(SORT_CYCLE, panel.value.sortMode);
      patch({ sortMode });
      return sortMode;
    },
    cycleProcessGroup() {
      const groupMode = nextInCycle(GROUP_CYCLE, panel.value.groupMode);
      patch({ groupMode });
      return groupMode;
    },
    setProcessFilter(text) { patch({ filterText: text.trim().toLowerCase() }); },
    setActiveDebuggeePid(pid) {
      if (panel.value.activeDebuggeePid !== pid) { patch({ activeDebuggeePid: pid }); }
    },
  };
}
