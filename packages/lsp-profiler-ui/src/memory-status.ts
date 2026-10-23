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
import { cmd, debugType, brand } from "./ids";
import { effect } from "@preact/signals-core";
import type { ProfilerStore as Store } from "./store";

/** Status bar priority — lower than host's main item. */
const MEMORY_STATUS_BAR_PRIORITY = 98;

let memoryStatusBarItem: vscode.StatusBarItem | undefined;
/** Last-rendered visibility, so the e2e seam knows when the item is hidden. */
let memoryStatusVisible = false;

/**
 * Create the memory status-bar item (click = action menu) and bind it to the
 * store's `profiler` signal and the debug-session lifecycle. Returns
 * disposables that tear down the effect, the listeners, and the item.
 */
export function bindMemoryStatusBar(store: Store): vscode.Disposable[] {
  memoryStatusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    MEMORY_STATUS_BAR_PRIORITY,
  );
  // Click the status-bar item to open the memory action menu (no palette needed).
  memoryStatusBarItem.command = cmd("memoryMenu");

  // Reactive leg: tracking state, and whether any runtime offers memory ([PROF-UI-CAPABILITIES]).
  const disposeEffect = effect(() => {
    void store.profiler.value.memory;
    void store.memoryOffered();
    refreshMemoryStatusBar(store);
  });

  refreshMemoryStatusBar(store);
  return [
    memoryStatusBarItem,
    { dispose: disposeEffect },
    // Imperative leg: show/hide as host debug sessions come and go.
    vscode.debug.onDidChangeActiveDebugSession(() => { refreshMemoryStatusBar(store); }),
    vscode.debug.onDidStartDebugSession(() => { refreshMemoryStatusBar(store); }),
    vscode.debug.onDidTerminateDebugSession(() => { refreshMemoryStatusBar(store); }),
    {
      dispose: () => {
        memoryStatusBarItem = undefined;
        memoryStatusVisible = false;
      },
    },
  ];
}

/**
 * Show the memory status-bar entry whenever a host debug session is active
 * and a runtime offers memory (or tracking is starting/on) and click it to open the action menu. Hidden
 * otherwise. A start in flight shows a spinner — never silence between the
 * click and the tracking state ([PROF-UI-STATE]).
 */
function refreshMemoryStatusBar(store: Store): void {
  if (memoryStatusBarItem === undefined) { return; }

  const debugging = vscode.debug.activeDebugSession?.type === debugType();
  const memoryState = store.profiler.value.memory;
  // A host whose runtimes advertise no memory backend never offers tracking.
  if ((!debugging || !store.memoryOffered()) && memoryState === "idle") {
    hideMemoryStatusBar();
    return;
  }

  if (memoryState === "starting") {
    memoryStatusBarItem.text = "$(loading~spin) Memory: starting…";
    memoryStatusBarItem.tooltip = `${brand()} is starting memory tracking`;
    memoryStatusBarItem.backgroundColor = undefined;
  } else if (memoryState === "active") {
    memoryStatusBarItem.text = "$(eye) Memory: tracking";
    memoryStatusBarItem.tooltip = `${brand()}: memory tracking active — click for snapshot/compare/stop`;
    memoryStatusBarItem.backgroundColor = new vscode.ThemeColor("statusBarItem.warningBackground");
  } else {
    memoryStatusBarItem.text = "$(database) Memory";
    memoryStatusBarItem.tooltip = `${brand()}: click to start memory tracking (briefly pauses the program)`;
    memoryStatusBarItem.backgroundColor = undefined;
  }
  memoryStatusVisible = true;
  memoryStatusBarItem.show();
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
