// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { brand, processUriScheme } from "./ids";
import type { ProcessInfo, RowRuntime } from "./process-explorer-rows";

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
  return `${String(rounded)} ${MEMORY_UNITS[exponent]}`;
}

/** Human-readable elapsed runtime, e.g. `3m`, `2h 14m`. */
function formatRuntime(seconds: number): string {
  if (seconds < SECONDS_PER_MINUTE) { return `${String(seconds)}s`; }
  if (seconds < SECONDS_PER_HOUR) { return `${String(Math.floor(seconds / SECONDS_PER_MINUTE))}m`; }
  const hours = Math.floor(seconds / SECONDS_PER_HOUR);
  const minutes = Math.floor((seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE);
  return `${String(hours)}h ${String(minutes)}m`;
}

/** Final path component, used to render a script's basename. */
function basename(path: string): string {
  const lastSep = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return lastSep >= 0 ? path.slice(lastSep + 1) : path;
}

/** 🚫 marker prefixed to the label of a process the profiler can't attach to. */
const BLOCKED_MARK = "🚫 ";

/** The live state one process row is constructed from. */
export interface ProcessTreeItemOptions {
  readonly activeProfilingPid?: number;
  readonly activeDebuggeePid?: number;
  readonly runtime?: RowRuntime;
}

/**
 * A single process row. Carries the visual cues for [PROF-UI-PROCESSES]:
 * a 🚫-prefixed, greyed, sunk row when not `debuggable`; a green row when
 * `inWorkspace`; a launcher chip; and the flame for the actively-profiled row.
 */
export class ProcessTreeItem extends vscode.TreeItem {
  public readonly process: ProcessInfo;
  public constructor(process: ProcessInfo, options: ProcessTreeItemOptions = {}) {
    super(rowLabel(process), vscode.TreeItemCollapsibleState.None);
    this.process = process;
    const rowRuntime = options.runtime ?? { directMemory: false, name: process.runtime, sourceEntries: false };
    // Stable identity across the 2s auto-refresh, or VS Code can fail to map an
    // inline-button click to a recreated element and pass no argument (#79).
    this.id = `process:${String(process.pid)}`;
    this.applyCues(process, rowRuntime, { activeProfilingPid: options.activeProfilingPid, activeDebuggeePid: options.activeDebuggeePid });
  }

  private applyCues(process: ProcessInfo, runtime: RowRuntime, pids: RowPids): void {
    const { profilingThis, tracksMemory, memoryTrackable } = rowState(process, runtime, pids);
    this.description = rowDescription(process, profilingThis);
    this.tooltip = rowTooltip(process, runtime.name, { profilingThis, memoryNeedsLaunch: tracksMemory && !memoryTrackable });
    this.iconPath = processIcon(process, profilingThis);
    this.contextValue = rowContextValue(process, profilingThis, memoryTrackable);
    // The decoration provider colours the label from this URI ([PROF-UI-PROCESSES]).
    this.resourceUri = processResourceUri(process);
    this.command = entryCommand(process, runtime);
  }
}

/** The row's label: name and entry script, 🚫-prefixed when it cannot be profiled. */
function rowLabel(process: ProcessInfo): string {
  const base = process.script === null ? process.name : `${process.name} — ${basename(process.script)}`;
  return process.debuggable ? base : `${BLOCKED_MARK}${base}`;
}

/** What a row is doing and offers. */
interface RowState {
  /** Being CPU-profiled: package.json swaps its inline Profile button for Stop ([PROF-UI-PROCESSES]). */
  readonly profilingThis: boolean;
  readonly tracksMemory: boolean;
  readonly memoryTrackable: boolean;
}

/** The two live PIDs a row's state compares against. */
interface RowPids {
  readonly activeProfilingPid: number | undefined;
  readonly activeDebuggeePid: number | undefined;
}

function rowState(process: ProcessInfo, runtime: RowRuntime, pids: RowPids): RowState {
  const tracksMemory = runtime.memory !== false;
  const debuggee = pids.activeDebuggeePid !== undefined && pids.activeDebuggeePid === process.pid;
  return {
    profilingThis: pids.activeProfilingPid !== undefined && pids.activeProfilingPid === process.pid,
    tracksMemory,
    memoryTrackable: tracksMemory && (debuggee || (runtime.directMemory && process.debuggable)),
  };
}

/**
 * Clicking opens the entry only when the runtime says it is source the editor
 * can show — never a native binary or an assembly ([PROF-UI-CAPABILITIES]).
 */
function entryCommand(process: ProcessInfo, runtime: RowRuntime): vscode.Command | undefined {
  if (process.script === null || !runtime.sourceEntries) { return undefined; }
  return { command: "vscode.open", title: "Reveal Script", arguments: [vscode.Uri.file(process.script)] };
}

/** A synthetic URI encoding the row's workspace + debuggability for decoration. */
function processResourceUri(process: ProcessInfo): vscode.Uri {
  const ws = process.inWorkspace ? "1" : "0";
  const dbg = process.debuggable ? "1" : "0";
  return vscode.Uri.from({ scheme: processUriScheme(), path: `/${String(process.pid)}`, query: `ws=${ws}&dbg=${dbg}` });
}

/** The launcher chip (`[uvicorn] `) for a row's description, or empty. */
function launcherChip(process: ProcessInfo): string {
  return process.launcher === null ? "" : `[${process.launcher}] `;
}

/** The description line: launcher chip, then the key live metrics. */
function rowDescription(process: ProcessInfo, profilingThis: boolean): string {
  const version = process.runtimeVersion ?? "—";
  const profilingSuffix = profilingThis ? " · profiling" : "";
  return `${launcherChip(process)}PID ${String(process.pid)} · ${version} · ${process.cpuPercent.toFixed(1)}% · ${formatBytes(process.memoryBytes)}${profilingSuffix}`;
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

/** The two booleans that shape a row's tooltip notices. */
interface RowNotices {
  readonly profilingThis: boolean;
  readonly memoryNeedsLaunch: boolean;
}

/** Multi-line hover tooltip surfacing every resolved detail for a process row. */
function rowTooltip(process: ProcessInfo, runtimeName: string, notices: RowNotices): string {
  return [...rowFacts(process, runtimeName), ...rowNotices(process, notices.profilingThis, notices.memoryNeedsLaunch)]
    .filter(Boolean).join("\n");
}

/** The process as resolved: identity, runtime, load and owner. */
function rowFacts(process: ProcessInfo, runtimeName: string): string[] {
  return [
    `${process.name} (PID ${String(process.pid)})`,
    process.interpreterPath === null ? "" : `Executable: ${process.interpreterPath}`,
    process.script === null ? "" : `Entry: ${process.script}`,
    `${runtimeName}: ${process.runtimeVersion ?? "unknown"}`,
    `CPU: ${process.cpuPercent.toFixed(1)}%  ·  Memory: ${formatBytes(process.memoryBytes)}`,
    `Runtime: ${formatRuntime(process.runtimeSecs)}`,
    process.user === null ? "" : `User: ${process.user}`,
    process.launcher === null ? "" : `Launcher: ${process.launcher}`,
  ];
}

/**
 * What the profiler can do with the process. An external process is profilable
 * only with elevation. A courier runtime tracks memory only on a process the
 * host launched ([PROF-UI-MEMORY]), so elsewhere the row points at the
 * current-file launch; a runtime with no memory backend suggests nothing
 * ([PROF-UI-CAPABILITIES]).
 */
function rowNotices(process: ProcessInfo, profilingThis: boolean, memoryNeedsLaunch: boolean): string[] {
  return [
    process.inWorkspace ? "📁 Workspace process" : "",
    profilingThis ? `🔥 ${brand()} is profiling this process — click Stop to finish` : "",
    !process.debuggable && process.undebuggableReason !== null ? `🚫 Can't profile — ${process.undebuggableReason}` : "",
    process.debuggable && process.requiresElevation ? "🔒 Profiling this process will prompt for elevation" : "",
    process.debuggable && memoryNeedsLaunch
      ? `🧠 Memory tracking needs the process under ${brand()} — use “Run & Track Memory (Current File)”`
      : "",
  ];
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
  const glyph = process.launcher === null ? "vm-running" : "rocket";
  return process.inWorkspace
    ? new vscode.ThemeIcon(glyph, new vscode.ThemeColor("charts.green"))
    : new vscode.ThemeIcon(glyph);
}
