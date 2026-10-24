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
import type { LanguageClient } from "vscode-languageclient/node";
import { cmd, viewId, brand } from "./ids";
import { Logger } from "./logger";
import type { ProfilerStore as Store } from "./store";
import { reportFailure } from "./failure";
import { withUserProgress } from "./progress-ops";
import {
  applyProfileDecorations,
  disposeProfileDecorations,
  type ProfileResult,
} from "./profiler-decorations";
import {
  disposeFlamegraphPanel,
  openFlamegraphWebview,
  presentProfileResult,
} from "./profiler-flamegraph-html";
import { disposeProfileServer } from "./profile-server";
import { CPU_CMD, handleProfileAttachToDebug, profileOnLaunch } from "./profiler-attach";
import { bindProfilerStatusBar, registerProgressListener } from "./profiler-status";
import { executeServerCommand } from "./server-command";
import { toProfileResult } from "./wire-adapters";
import type { ProfileReport } from "./wire-generated";

// Re-exported so tests keep one import site for the profiler's public seams.
export { profilerStatusText } from "./profiler-status";
export { shouldProfileOnLaunch } from "./profiler-launch";

// ── State ─────────────────────────────────────────────────────────────────
//
// Session state (is a profile running? which PID/session?) lives in the store
// as a reactive signal ([PROF-UI-PROCESSES]) so the status bar and the
// Processes panel react to it. Only the last result is cached here — a
// UI artifact (re-applied as decorations on editor focus), not session state.

let lastResult: ProfileResult | undefined;

/** The most recent CPU result the UI presented (e2e seam). */
export function lastProfileResult(): ProfileResult | undefined {
  return lastResult;
}

/** Keep `result` as the last one and paint its heat on the editors. */
function keepResult(result: ProfileResult): void {
  lastResult = result;
  applyProfileDecorations(result);
}

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
      if (lastResult !== undefined) { applyProfileDecorations(lastResult); }
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
    vscode.commands.registerCommand(cmd("profileShowResults"), () => { handleProfileShowResults(); }),
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

// ── Command handlers ──────────────────────────────────────────────────────

async function handleProfileStart(): Promise<void> {
  // [PROF-UI-PROCESSES] #62: the profiler no longer asks the user to
  // hand-type a PID (and there was never a real "auto-detect"). Reveal the
  // Processes panel so they can pick a process — or run & profile the
  // current file — and start profiling with one click.
  await vscode.commands.executeCommand(`${viewId()}.focus`);
  vscode.window.showInformationMessage(
    `${brand()}: Pick a process in the panel to profile, or run & profile the current file.`,
  );
}

/** The running client and the active CPU session, or a warning that there is none. */
interface ActiveSession {
  readonly client: LanguageClient;
  readonly sessionId: string;
}

function activeSession(store: Store): ActiveSession | undefined {
  const client = store.client.value;
  const sessionId = store.profiler.value.cpuSessionId;
  if (client?.isRunning() !== true || sessionId === undefined) {
    vscode.window.showWarningMessage(`${brand()}: No active profiling session.`);
    return undefined;
  }
  return { client, sessionId };
}

/**
 * Ask the server for the active session's report under a progress
 * notification: collecting samples and writing artifacts takes a beat
 * ([PROF-UI-STATE]).
 */
async function collectReport(active: ActiveSession, command: string, title: string, step: string): Promise<ProfileResult | undefined> {
  return await withUserProgress(title, async (report) => {
    report(step);
    const raw = await executeServerCommand<ProfileReport>(active.client, command, { session_id: active.sessionId });
    return raw === undefined ? undefined : toProfileResult(raw);
  });
}

async function handleProfileStop(store: Store): Promise<void> {
  const active = activeSession(store);
  if (active === undefined) { return; }
  try {
    const result = await collectReport(active, CPU_CMD.stop, `${brand()}: Stopping profiler`, "Collecting results…");
    store.profilerStopped();
    if (result === undefined) { return; }
    keepResult(result);
    // Land the user on the self-contained results panel, beside the
    // heat-mapped source; the raw `.cpuprofile` stays one click away
    // ([PROF-CPU-START], #145).
    presentProfileResult(result);
  } catch (err: unknown) {
    store.profilerStopped();
    reportFailure("Profile stop", err);
  }
}

async function handleProfileSnapshot(store: Store): Promise<void> {
  const active = activeSession(store);
  if (active === undefined) { return; }
  try {
    const result = await collectReport(active, CPU_CMD.snapshot, `${brand()}: Taking profile snapshot`, "Collecting samples so far…");
    if (result !== undefined) { announceSnapshot(result); }
  } catch (err: unknown) {
    reportFailure("Profile snapshot", err);
  }
}

/**
 * Keep a snapshot's result and offer its panel. Fire-and-forget: a toast with
 * a button is sticky and must not block the snapshot handler ([PROF-CPU-START]).
 */
function announceSnapshot(result: ProfileResult): void {
  keepResult(result);
  Logger.info(`Profile snapshot: ${String(result.totalSamples)} samples so far`);
  void vscode.window
    .showInformationMessage(`${brand()}: Snapshot — ${String(result.totalSamples)} samples (profiling continues)`, "View Results")
    .then((choice) => {
      if (choice === "View Results") { openFlamegraphWebview(result); }
    });
}

/**
 * Re-open the results panel for the most recent profile. The panel is a
 * singleton the user can close; this palette command is the always-available
 * way back in — results are never trapped behind a dismissed completion toast
 * ([PROF-CPU-START]).
 */
function handleProfileShowResults(): void {
  if (lastResult === undefined) {
    vscode.window.showInformationMessage(
      `${brand()}: No profile results yet — run a profiling session first.`,
    );
    return;
  }
  openFlamegraphWebview(lastResult);
}

// ── Disposal ──────────────────────────────────────────────────────────────

/**
 * Dispose the profiler's UI artifacts. Session state lives in the store and is
 * cleared by `store.reset()` on deactivation; the status bar and context keys
 * follow reactively, so there is nothing imperative to tear down here.
 */
export function disposeProfiler(): void {
  disposeProfileDecorations();
  disposeFlamegraphPanel();
  disposeProfileServer();
  lastResult = undefined;
}
