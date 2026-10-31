// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Processes panel — row presentation layer.
 *
 * Everything about how a process renders as a tree row lives here: the process
 * model, sort/group mode tables, and the tree-item classes. The item classes
 * are split into sibling modules (one class per file) and re-exported here so
 * the provider keeps one import site.
 */

import type { LaunchActionItem } from "./process-launch-item";
import type { MessageTreeItem } from "./process-message-item";
import type { ProcessGroupItem } from "./process-group-item";
import type { ProcessTreeItem } from "./process-tree-item";

export { LaunchActionItem } from "./process-launch-item";
export { MessageTreeItem } from "./process-message-item";
export { ProcessGroupItem } from "./process-group-item";
export { ProcessTreeItem } from "./process-tree-item";

// ── Process model ─────────────────────────────────────────────────────────

/** What the server advertises about a row's runtime ([PROF-UI-CAPABILITIES]). */
export interface RowRuntime {
  /** The runtime reads its own heap, so any attachable process can be memory-tracked. */
  readonly directMemory: boolean;
  /** The runtime's advertised `display_name`; the wire id when the server named none. */
  readonly name: string;
  /** The process's entry is a source file the editor can open (never a binary). */
  readonly sourceEntries: boolean;
  /** False when the runtime has no memory backend: the row then offers and suggests no memory tracking. */
  readonly memory?: boolean;
}

/** One recognised process; the view model of the wire `ProcessInfo` ([PROF-PROCESSES]). */
export interface ProcessInfo {
  readonly pid: number;
  readonly ppid: number;
  readonly name: string;
  readonly interpreterPath: string | null;
  readonly script: string | null;
  readonly runtime: string;
  readonly runtimeVersion: string | null;
  readonly cpuPercent: number;
  readonly memoryBytes: number;
  readonly runtimeSecs: number;
  readonly user: string | null;
  readonly requiresElevation: boolean;
  /** `true` when the process belongs to an open workspace root — renders green. */
  readonly inWorkspace: boolean;
  /** Framework name (`uvicorn`, …) when a known launcher, else `null` — chip. */
  readonly launcher: string | null;
  /** `false` when the profiler can't attach — renders 🚫, greyed, sorted last. */
  readonly debuggable: boolean;
  /** Tooltip reason shown when `debuggable` is `false`. */
  readonly undebuggableReason: string | null;
}

// ── Sort / group modes ───────────────────────────────────────────────────

export type SortMode = "cpu" | "memory" | "pid" | "name" | "runtime" | "version";

export const SORT_CYCLE: readonly SortMode[] = ["cpu", "memory", "pid", "name", "runtime", "version"];

export const SORT_LABEL: Readonly<Record<SortMode, string>> = {
  cpu: "CPU %",
  memory: "Memory",
  pid: "PID",
  name: "Name",
  runtime: "Runtime",
  version: "Runtime version",
};

/** Per-mode row comparators; `sortProcesses` wraps these to sink non-debuggable rows. */
export const SORT_COMPARATORS: Readonly<Record<SortMode, (left: ProcessInfo, right: ProcessInfo) => number>> = {
  cpu: (left, right) => right.cpuPercent - left.cpuPercent,
  memory: (left, right) => right.memoryBytes - left.memoryBytes,
  pid: (left, right) => left.pid - right.pid,
  name: (left, right) => left.name.localeCompare(right.name),
  runtime: (left, right) => right.runtimeSecs - left.runtimeSecs,
  version: (left, right) => (left.runtimeVersion ?? "").localeCompare(right.runtimeVersion ?? ""),
};

export type GroupMode = "none" | "version" | "interpreter" | "user" | "parent";

export const GROUP_CYCLE: readonly GroupMode[] = ["none", "version", "interpreter", "user", "parent"];

export const GROUP_LABEL: Readonly<Record<GroupMode, string>> = {
  none: "None",
  version: "Runtime version",
  interpreter: "Executable",
  user: "User",
  parent: "Parent process",
};

export type TreeItem = LaunchActionItem | ProcessGroupItem | ProcessTreeItem | MessageTreeItem;
