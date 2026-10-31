// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Profiler UI module for the lsp-profiler UI library.
 *
 * Provides:
 * - Profiler status bar item (pulsing dot while profiling)
 * - Command handlers for start/stop/snapshot/attach-to-debug
 * - Flamegraph webview panel
 * - Progress notification during active profiling
 *
 * All profiling logic lives in the LSP server. This module handles
 * only the client-side UI and command routing; starts live in profiler-attach.ts.
 */

import * as vscode from "vscode";
import { cmd } from "./ids";
import { Logger } from "./logger";
import type { ProfilerStore as Store } from "./store";
import { applyProfileDecorations } from "./profiler-decorations";
import { handleProfileAttachToDebug, profileOnLaunch } from "./profiler-attach";
import { bindProfilerStatusBar, registerProgressListener } from "./profiler-status";
import { handleProfileShowResults, lastProfileResult } from "./profiler-results";
import { handleProfileSnapshot, handleProfileStart, handleProfileStop } from "./profiler-commands";

// Re-exported so tests keep one import site for the profiler's public seams.
export { profilerStatusText } from "./profiler-status";
export { shouldProfileOnLaunch } from "./profiler-launch";
export { lastProfileResult } from "./profiler-results";
export { disposeProfiler } from "./profiler-dispose";

// ── Registration ──────────────────────────────────────────────────────────

/**
 * Register profiler UI components. Called once during extension activation.
 * Returns disposables for cleanup.
 */
export function registerProfiler(store: Store): vscode.Disposable[] {
  return [
    // The status bar renders reactively from the store's profiler signal
    // ([PROF-UI-STATE] lifecycle lives in profiler-status.ts).
    bindProfilerStatusBar(store),
    ...registerProfilerCommands(store),
    registerProgressListener(store),
    // Decorations follow the last result onto every newly visible editor.
    vscode.window.onDidChangeVisibleTextEditors(() => {
      const result = lastProfileResult();
      if (result !== undefined) { void applyProfileDecorations(result); }
    }),
    vscode.debug.onDidStartDebugSession((session) => { profileOnLaunch(store, session); }),
    vscode.debug.onDidTerminateDebugSession(() => { stopWithDebugSession(store); }),
  ];
}

/** The client-side commands that proxy to the LSP. */
function registerProfilerCommands(store: Store): vscode.Disposable[] {
  return [
    vscode.commands.registerCommand(cmd("profileStart"), async () => { await handleProfileStart(); }),
    vscode.commands.registerCommand(cmd("profileStop"), async () => { await handleProfileStop(store); }),
    vscode.commands.registerCommand(cmd("profileSnapshot"), async () => { await handleProfileSnapshot(store); }),
    vscode.commands.registerCommand(cmd("profileAttachToDebug"), async () => { await handleProfileAttachToDebug(store); }),
    vscode.commands.registerCommand(cmd("profileShowResults"), handleProfileShowResults),
  ];
}

/**
 * Auto-stop when the debug session ends. Only an adopted ("active") session has
 * results to collect; a start still in flight is cleaned up by its own `finally`,
 * so no "no session" warning fires.
 */
function stopWithDebugSession(store: Store): void {
  if (store.profiler.value.cpu !== "active") { return; }
  Logger.info("Debug session ended — auto-stopping profiler");
  void handleProfileStop(store);
}
