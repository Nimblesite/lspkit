// Implements [PROF-UI-STATE] + [PROF-UI-PROCESSES].
// See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Memory status-bar lifecycle: hidden → one-click start affordance (while a
 * host debug session is active) → starting spinner → tracking readout.
 * A start in flight is never silent ([PROF-UI-STATE]).
 *
 * The item renders from the store's reactive `profiler` signal plus the debug
 * session lifecycle (the "debugging but not yet tracking" state is not a store
 * signal), mirroring `profiler-status.ts`. Owned here so memory-profiler.ts
 * stays focused on command routing and session flow.
 */

import * as vscode from "vscode";
import { brand, cmd, debugType } from "./ids";
import { effect } from "@preact/signals-core";
import type { ProfilerStore as Store } from "./store";

/** Status bar priority — lower than host's main item. */
const MEMORY_STATUS_BAR_PRIORITY = 98;

let memoryStatusBarItem: vscode.StatusBarItem | null = null;
/** Last-rendered visibility, so the e2e seam knows when the item is hidden. */
let memoryStatusVisible = false;

/**
 * Create the memory status-bar item (click = action menu) and bind it to the
 * store's `profiler` signal and the debug-session lifecycle. Returns
 * disposables that tear down the effect, the listeners, and the item.
 */
export function bindMemoryStatusBar(store: Store): vscode.Disposable[] {
  memoryStatusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, MEMORY_STATUS_BAR_PRIORITY);
  // Click the status-bar item to open the memory action menu (no palette needed).
  memoryStatusBarItem.command = cmd("memoryMenu");
  // Reactive leg: tracking state, and whether any runtime offers memory ([PROF-UI-CAPABILITIES]).
  const disposeEffect = effect(() => { refreshMemoryStatusBar(store); });
  refreshMemoryStatusBar(store);
  const forget = (): void => { memoryStatusBarItem = null; memoryStatusVisible = false; };
  return [memoryStatusBarItem, { dispose: disposeEffect }, ...debugSessionRefresh(store), { dispose: forget }];
}

/** The imperative leg: show/hide as host debug sessions come and go. */
function debugSessionRefresh(store: Store): vscode.Disposable[] {
  const refresh = (): void => { refreshMemoryStatusBar(store); };
  return [
    vscode.debug.onDidChangeActiveDebugSession(refresh),
    vscode.debug.onDidStartDebugSession(refresh),
    vscode.debug.onDidTerminateDebugSession(refresh),
  ];
}

/**
 * Show the memory status-bar entry whenever a host debug session is active
 * and a runtime offers memory (or tracking is starting/on) and click it to open the action menu. Hidden
 * otherwise. A start in flight shows a spinner — never silence between the
 * click and the tracking state ([PROF-UI-STATE]).
 */
function refreshMemoryStatusBar(store: Store): void {
  if (memoryStatusBarItem === null) { return; }

  // Both signals are read on every pass, so the effect that calls this
  // re-runs when either changes ([PROF-UI-CAPABILITIES]).
  const memoryState = store.profiler.value.memory;
  const offered = store.memoryOffered();
  const debugging = vscode.debug.activeDebugSession?.type === debugType();
  // A host whose runtimes advertise no memory backend never offers tracking.
  if ((!debugging || !offered) && memoryState === "idle") {
    hideMemoryStatusBar();
    return;
  }
  showMemoryStatusBar(memoryStatusBarItem, memoryStatusLook(memoryState));
}

/** Paint the item with `look` and show it. */
function showMemoryStatusBar(item: vscode.StatusBarItem, look: { text: string; tooltip: string; highlighted: boolean }): void {
  item.text = look.text;
  item.tooltip = look.tooltip;
  item.backgroundColor = look.highlighted ? new vscode.ThemeColor("statusBarItem.warningBackground") : undefined;
  memoryStatusVisible = true;
  item.show();
}

/** The item's text and tooltip in each tracking state; live tracking is highlighted. */
function memoryStatusLook(state: string): { text: string; tooltip: string; highlighted: boolean } {
  if (state === "starting") {
    return { text: "$(loading~spin) Memory: starting…", tooltip: `${brand()} is starting memory tracking`, highlighted: false };
  }
  if (state === "active") {
    return { text: "$(eye) Memory: tracking", tooltip: `${brand()}: memory tracking active — click for snapshot/compare/stop`, highlighted: true };
  }
  return { text: "$(database) Memory", tooltip: `${brand()}: click to start memory tracking (briefly pauses the program)`, highlighted: false };
}

/** Hide the item and record it for the e2e seam. */
function hideMemoryStatusBar(): void {
  memoryStatusVisible = false;
  memoryStatusBarItem?.hide();
}

/**
 * The memory status-bar text, or undefined when hidden. E2e seam for
 * [PROF-UI-STATE]: StatusBarItem visibility is not readable via the
 * public API, mirroring `profilerStatusText()` (profiler-status.ts).
 */
export function memoryStatusText(): string | undefined {
  return memoryStatusVisible ? memoryStatusBarItem?.text : undefined;
}
