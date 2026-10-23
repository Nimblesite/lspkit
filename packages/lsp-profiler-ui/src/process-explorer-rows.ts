// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Processes panel — row presentation layer.
 *
 * Everything about how a process renders as a tree row lives here: the process
 * model, sort/group mode tables, formatting helpers, the tree-item classes, and
 * the row cue builders (description, tooltip, icon, contextValue, decoration
 * URI). The provider (fetching, sorting, grouping, registration) lives in
 * `process-explorer.ts`; split to keep both under the 500-LOC file limit.
 */

import * as vscode from "vscode";
import { brand, processUriScheme } from "./ids";

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
export const SORT_COMPARATORS: Readonly<Record<SortMode, (a: ProcessInfo, b: ProcessInfo) => number>> = {
  cpu: (a, b) => b.cpuPercent - a.cpuPercent,
  memory: (a, b) => b.memoryBytes - a.memoryBytes,
  pid: (a, b) => a.pid - b.pid,
  name: (a, b) => a.name.localeCompare(b.name),
  runtime: (a, b) => b.runtimeSecs - a.runtimeSecs,
  version: (a, b) => (a.runtimeVersion ?? "").localeCompare(b.runtimeVersion ?? ""),
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

// ── Formatting helpers ───────────────────────────────────────────────────

const BYTES_PER_UNIT = 1024;
const MEMORY_UNITS: readonly string[] = ["B", "KB", "MB", "GB", "TB"];
const SECONDS_PER_MINUTE = 60;
const SECONDS_PER_HOUR = 3600;

/** Human-readable memory size, e.g. `88 MB`. */
function formatBytes(bytes: number): string {
  if (bytes <= 0) { return "0 B"; }
  const exponent = Math.min(
    Math.floor(Math.log(bytes) / Math.log(BYTES_PER_UNIT)),
    MEMORY_UNITS.length - 1,
  );
  const value = bytes / BYTES_PER_UNIT ** exponent;
  const rounded = exponent === 0 ? value : Math.round(value);
  return `${rounded} ${MEMORY_UNITS[exponent]}`;
}

/** Human-readable elapsed runtime, e.g. `3m`, `2h 14m`. */
function formatRuntime(seconds: number): string {
  if (seconds < SECONDS_PER_MINUTE) { return `${seconds}s`; }
  if (seconds < SECONDS_PER_HOUR) { return `${Math.floor(seconds / SECONDS_PER_MINUTE)}m`; }
  const hours = Math.floor(seconds / SECONDS_PER_HOUR);
  const minutes = Math.floor((seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE);
  return `${hours}h ${minutes}m`;
}

/** Final path component, used to render a script's basename. */
function basename(path: string): string {
  const lastSep = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return lastSep >= 0 ? path.slice(lastSep + 1) : path;
}

// ── Tree items ───────────────────────────────────────────────────────────

export type TreeItem = LaunchActionItem | ProcessGroupItem | ProcessTreeItem | MessageTreeItem;

/**
 * A non-process placeholder row. Returned (instead of an empty list) when the
 * user's search filter empties a NON-empty process list, so the empty-tree
 * viewsWelcome never claims "No processes running" while processes are in
 * fact running, just filtered (procexp-2).
 */
export class MessageTreeItem extends vscode.TreeItem {
  constructor(label: string) {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.contextValue = "processesMessage";
    this.iconPath = new vscode.ThemeIcon("filter");
  }
}

/**
 * A persistent "Run & …(Current File)" launch row pinned to the top of the tree.
 * VS Code only renders the big `viewsWelcome` buttons when the tree is EMPTY, so
 * once any process appears those buttons vanish — these rows keep the current-file
 * launches reachable alongside a populated list ([PROF-UI-PROCESSES]).
 * Gated per-activity (hidden while the matching metric is busy) like the title-bar
 * buttons, so a second same-metric run is never offered ([PROF-UI-PROCESSES]).
 */
export class LaunchActionItem extends vscode.TreeItem {
  constructor(label: string, command: string, icon: string) {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.id = `launchAction:${command}`;
    this.contextValue = "launchAction";
    this.iconPath = new vscode.ThemeIcon(icon, new vscode.ThemeColor("textLink.foreground"));
    this.command = { command, title: label };
  }
}

/** A collapsible group header (when grouping is active). */
export class ProcessGroupItem extends vscode.TreeItem {
  constructor(
    public readonly label: string,
    public readonly members: readonly ProcessInfo[],
  ) {
    super(label, vscode.TreeItemCollapsibleState.Expanded);
    // Stable identity so VS Code preserves expansion state across refreshes.
    this.id = `processGroup:${label}`;
    this.description = `${members.length}`;
    this.contextValue = "processGroup";
    this.iconPath = new vscode.ThemeIcon("folder");
  }
}

/** 🚫 marker prefixed to the label of a process the profiler can't attach to. */
const BLOCKED_MARK = "🚫 ";

/**
 * A single process row. Carries the visual cues for [PROF-UI-PROCESSES]:
 * a 🚫-prefixed, greyed, sunk row when not `debuggable`; a green row when
 * `inWorkspace`; a launcher chip; and the flame for the actively-profiled row.
 */
export class ProcessTreeItem extends vscode.TreeItem {
  constructor(
    public readonly process: ProcessInfo,
    activeProfilingPid?: number,
    activeDebuggeePid?: number,
    runtime: RowRuntime = { directMemory: false, name: process.runtime, sourceEntries: false },
  ) {
    const scriptName = process.script !== null ? basename(process.script) : undefined;
    const base = scriptName !== undefined ? `${process.name} — ${scriptName}` : process.name;
    super(
      process.debuggable ? base : `${BLOCKED_MARK}${base}`,
      vscode.TreeItemCollapsibleState.None,
    );

    // Stable identity across the panel's 2s auto-refresh: without it VS Code
    // can fail to map an inline-button click back to a (recreated) element and
    // invokes the command with no argument (#79).
    this.id = `process:${process.pid}`;

    // The row currently being CPU-profiled gets a distinct look + contextValue
    // so package.json swaps its inline Profile button for a Stop button
    // ([PROF-UI-PROCESSES]).
    const profilingThis = activeProfilingPid !== undefined && activeProfilingPid === process.pid;
    // A courier runtime can only track the active debuggee (its heap is read
    // through the DAP courier — [PROF-UI-MEMORY]); a direct runtime reads any
    // attachable heap ([PROF-UI-CAPABILITIES]). Trackable rows get the
    // contextValue that reveals the inline Track Memory action, hidden
    // everywhere else so the action is never offered where it would refuse.
    const tracksMemory = runtime.memory !== false;
    const memoryTrackable = tracksMemory && ((activeDebuggeePid !== undefined && activeDebuggeePid === process.pid) || (runtime.directMemory && process.debuggable));
    this.description = rowDescription(process, profilingThis);
    this.tooltip = rowTooltip(process, runtime.name, profilingThis, tracksMemory && !memoryTrackable);
    this.iconPath = processIcon(process, profilingThis);
    this.contextValue = rowContextValue(process, profilingThis, memoryTrackable);
    // The FileDecorationProvider keys off this synthetic URI to colour the whole
    // label green (workspace) or grey (non-debuggable) — [PROF-UI-PROCESSES].
    this.resourceUri = processResourceUri(process);

    // Clicking opens the entry only when the runtime says it is source the
    // editor can show — never a native binary or an assembly ([PROF-UI-CAPABILITIES]).
    if (process.script !== null && runtime.sourceEntries) {
      this.command = {
        command: "vscode.open",
        title: "Reveal Script",
        arguments: [vscode.Uri.file(process.script)],
      };
    }
  }
}

/** Scheme of the synthetic per-row URI the decoration provider colours. */

/** A synthetic URI encoding the row's workspace + debuggability for decoration. */
function processResourceUri(process: ProcessInfo): vscode.Uri {
  const ws = process.inWorkspace ? "1" : "0";
  const dbg = process.debuggable ? "1" : "0";
  return vscode.Uri.from({ scheme: processUriScheme(), path: `/${process.pid}`, query: `ws=${ws}&dbg=${dbg}` });
}

/** The launcher chip (`[uvicorn] `) for a row's description, or empty. */
function launcherChip(process: ProcessInfo): string {
  return process.launcher !== null ? `[${process.launcher}] ` : "";
}

/** The description line: launcher chip, then the key live metrics. */
function rowDescription(process: ProcessInfo, profilingThis: boolean): string {
  const version = process.runtimeVersion ?? "—";
  const profilingSuffix = profilingThis ? " · profiling" : "";
  return `${launcherChip(process)}PID ${process.pid} · ${version} · ${process.cpuPercent.toFixed(1)}% · ${formatBytes(process.memoryBytes)}${profilingSuffix}`;
}

/**
 * The row's contextValue, which selects its package.json affordances:
 * `blockedProcess` (no Profile/Track actions — the panel already says
 * "Can't profile", so the menus must not offer an attach that is known to fail,
 * #266), `processProfiling` (Stop), `processDebuggee` (Track Memory
 * enabled), `processElevated` (lock), or plain `process`. The
 * profiling clauses match `/^process/`, which `blockedProcess`
 * deliberately does not.
 */
function rowContextValue(process: ProcessInfo, profilingThis: boolean, memoryTrackable: boolean): string {
  if (!process.debuggable) { return "blockedProcess"; }
  if (profilingThis) { return "processProfiling"; }
  if (memoryTrackable) { return "processDebuggee"; }
  return process.requiresElevation ? "processElevated" : "process";
}

/** Multi-line hover tooltip surfacing every resolved detail for a process row. */
function rowTooltip(process: ProcessInfo, runtimeName: string, profilingThis: boolean, memoryNeedsLaunch: boolean): string {
  return [
    `${process.name} (PID ${process.pid})`,
    process.interpreterPath !== null ? `Executable: ${process.interpreterPath}` : "",
    process.script !== null ? `Entry: ${process.script}` : "",
    `${runtimeName}: ${process.runtimeVersion ?? "unknown"}`,
    `CPU: ${process.cpuPercent.toFixed(1)}%  ·  Memory: ${formatBytes(process.memoryBytes)}`,
    `Runtime: ${formatRuntime(process.runtimeSecs)}`,
    process.user !== null ? `User: ${process.user}` : "",
    process.launcher !== null ? `Launcher: ${process.launcher}` : "",
    process.inWorkspace ? "📁 Workspace process" : "",
    profilingThis ? `🔥 ${brand()} is profiling this process — click Stop to finish` : "",
    !process.debuggable && process.undebuggableReason !== null
      ? `🚫 Can't profile — ${process.undebuggableReason}`
      : "",
    // An external process is profilable only with elevation.
    process.debuggable && process.requiresElevation
      ? "🔒 Profiling this process will prompt for elevation"
      : "",
    // A courier runtime tracks memory only on a process the host launched
    // ([PROF-UI-MEMORY]); elsewhere point at the current-file launch. A runtime
    // with no memory backend suggests nothing ([PROF-UI-CAPABILITIES]).
    process.debuggable && memoryNeedsLaunch
      ? `🧠 Memory tracking needs the process under ${brand()} — use “Run & Track Memory (Current File)”`
      : "",
  ].filter(Boolean).join("\n");
}

/**
 * Leading icon for a process row: the flame while profiling, `circle-slash` when
 * not debuggable, a `lock` when it needs elevation, `rocket` for launchers, else
 * a running-VM glyph. In-workspace debuggable rows are tinted green to match the
 * label decoration.
 */
function processIcon(process: ProcessInfo, profilingThis: boolean): vscode.ThemeIcon {
  if (profilingThis) {
    return new vscode.ThemeIcon("flame", new vscode.ThemeColor("statusBarItem.warningBackground"));
  }
  if (!process.debuggable) {
    return new vscode.ThemeIcon("circle-slash", new vscode.ThemeColor("disabledForeground"));
  }
  if (process.requiresElevation) {
    return new vscode.ThemeIcon("lock", new vscode.ThemeColor("list.warningForeground"));
  }
  const glyph = process.launcher !== null ? "rocket" : "vm-running";
  return process.inWorkspace
    ? new vscode.ThemeIcon(glyph, new vscode.ThemeColor("charts.green"))
    : new vscode.ThemeIcon(glyph);
}
