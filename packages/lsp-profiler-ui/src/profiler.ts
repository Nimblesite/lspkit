// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Profiler UI module for the lsp-profiler UI library.
 *
 * Provides:
 * - Profiler status bar item (pulsing orange dot while profiling)
 * - Command handlers for start/stop/snapshot/attach-to-debug
 * - Flamegraph webview panel
 * - Progress notification during active profiling
 *
 * All profiling logic lives in the LSP server. This module handles
 * only the client-side UI and command routing.
 */

import * as vscode from "vscode";
import { lspTable, cmd, viewId, configSection, brand } from "./ids";
import { Logger } from "./logger";
import type { ProfilerStore as Store } from "./store";
import { evaluateInDebugSession, waitForStoppedFrame } from "./dap-evaluate";
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
import { shouldProfileOnLaunch, waitForDebuggeePid } from "./profiler-launch";
import { bindProfilerStatusBar, registerProgressListener } from "./profiler-status";
import { toProfileResult, toStartedSession, type StartedSession } from "./wire-adapters";
import type { CooperativeScript, ProfileReport, StartProfileResult } from "./wire-generated";

// Re-exported so tests keep one import site for the profiler's public seams.
export { profilerStatusText } from "./profiler-status";
export { shouldProfileOnLaunch } from "./profiler-launch";

// ── Constants ─────────────────────────────────────────────────────────────

/** LSP command names ([PROF-WIRE-METHODS]). */
const LSP_CMD = lspTable({
  start: "cpu.start",
  stop: "cpu.stop",
  snapshot: "cpu.snapshot",
  list: "cpu.list",
  cooperativeScript: "cpu.cooperativeScript",
  cooperativeAttach: "cpu.cooperativeAttach",
});

/** Ack printed by the injected cooperative sampler ([PROF-CPU-COOPERATIVE]). */
const COOPERATIVE_ACK = "__LSPPROF_CPU_ACK__";

// ── Constants ─────────────────────────────────────────────────────────────

/** Default sample rate fallback (Hz). */
const DEFAULT_SAMPLE_RATE = 100;

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

// ── Registration ──────────────────────────────────────────────────────────

/**
 * Register profiler UI components. Called once during extension activation.
 * Returns disposables for cleanup.
 */
export function registerProfiler(store: Store): vscode.Disposable[] {
  const disposables: vscode.Disposable[] = [];

  // Status bar item — renders reactively from the store's profiler signal
  // ([PROF-UI-STATE] lifecycle lives in profiler-status.ts).
  disposables.push(bindProfilerStatusBar(store));

  // Client-side commands that proxy to LSP.
  disposables.push(
    vscode.commands.registerCommand(cmd("profileStart"), async () => handleProfileStart()),
    vscode.commands.registerCommand(cmd("profileStop"), async () => handleProfileStop(store)),
    vscode.commands.registerCommand(cmd("profileSnapshot"), async () => handleProfileSnapshot(store)),
    vscode.commands.registerCommand(cmd("profileAttachToDebug"), async () => handleProfileAttachToDebug(store)),
    vscode.commands.registerCommand(cmd("profileShowResults"), () => { handleProfileShowResults(); }),
  );

  // Listen for profiler progress notifications from LSP.
  disposables.push(registerProgressListener(store));

  // Clear decorations when active editor changes (optional, re-applies on focus).
  disposables.push(
    vscode.window.onDidChangeVisibleTextEditors(() => {
      if (lastResult !== undefined) {
        applyProfileDecorations(lastResult);
      }
    }),
  );

  // "Profile on Launch" — automatically start profiling when a debug session starts.
  disposables.push(
    vscode.debug.onDidStartDebugSession((session) => {
      if (shouldProfileOnLaunch(session) && store.profiler.value.cpu === "idle") {
        Logger.info(`Profile on Launch: auto-profiling debug session ${session.id}`);
        notifyBreakpointsSuppressedForProfiling();
        void startProfilerOnLaunch(store, session.id);
      }
    }),
  );

  // Auto-stop profiling when debug session ends. Only an adopted ("active")
  // session has results to collect; a start still in flight is cleaned up by
  // startProfilerOnLaunch's own finally, so don't fire a "no session" warning.
  disposables.push(
    vscode.debug.onDidTerminateDebugSession(() => {
      if (store.profiler.value.cpu === "active") {
        Logger.info("Debug session ended — auto-stopping profiler");
        void handleProfileStop(store);
      }
    }),
  );

  return disposables;
}

// ── Session adoption ──────────────────────────────────────────────────────


/**
 * Adopt a freshly started session into the shared reactive state. Writing it to
 * the store drives the status bar, the panel chrome + button gating, and the
 * stop/snapshot/progress routing — all from one signal
 * ([PROF-UI-PROCESSES]).
 */
function adoptSession(store: Store, result: StartedSession, announcement: string): void {
  store.profilerActive(result.pid, result.sessionId);
  Logger.info(
    `Profiling started: PID ${result.pid}, ${result.runtime} ${result.runtimeVersion}, session ${result.sessionId}`,
  );
  vscode.window.showInformationMessage(announcement);
}

/**
 * A "stop the current CPU session first" warning. Only the CPU leg blocks a CPU
 * start ([PROF-UI-PROCESSES]), so this names the active CPU profile.
 */
function busyMessage(store: Store): string {
  const session = store.profiler.value;
  const pid = session.cpuPid !== undefined ? ` PID ${session.cpuPid}` : "";
  return `${brand()}: Already profiling${pid}. Stop the current CPU session first.`;
}

// ── Launch flows ([PROF-CPU-COOPERATIVE], [PROF-UI-STATE]) ───────────

/** The single progress title every CPU-start flow shares. */
function cpuStartTitle(): string {
  return `${brand()}: Starting CPU profiler`;
}

/** Shown once per session: a profiling launch neutralises breakpoints (ux-6). */
let breakpointSuppressionNoticeShown = false;

/**
 * Tell the user, once, that a profiling launch runs to completion with their
 * breakpoints disabled — otherwise a Run & Profile (or a plain F5 with the
 * global `profiler.profileOnLaunch` setting on) silently never stops at a
 * breakpoint, which is baffling while the gutter still shows them armed (ux-6).
 * Only fires when breakpoints are actually set, so a breakpoint-free run is
 * never narrated.
 */
function notifyBreakpointsSuppressedForProfiling(): void {
  if (breakpointSuppressionNoticeShown || vscode.debug.breakpoints.length === 0) {
    return;
  }
  breakpointSuppressionNoticeShown = true;
  void vscode.window.showInformationMessage(
    `${brand()}: Profiling run — your breakpoints are disabled so the program runs to completion. Launch without profiling to debug with breakpoints.`,
  );
}

/**
 * Auto-start dispatcher for a freshly launched debug session: cooperative
 * sampler on macOS, py-spy attach elsewhere. The whole start runs under one
 * progress notification with stage messages, and the status bar shows a
 * spinner until the live sample counter takes over — a click is never
 * followed by silence ([PROF-UI-STATE]).
 */
async function startProfilerOnLaunch(store: Store, debugSessionId: string): Promise<void> {
  store.profilerStarting();
  try {
    await withUserProgress(cpuStartTitle(), async (report) => {
      if (process.platform === "darwin" && cpuIsCooperativeEverywhere(store)) {
        // macOS gates task ports behind entitled debuggers, so a runtime that
        // can only be sampled from outside through them (py-spy gets EPERM
        // even as root) is sampled by its cooperative in-process sampler,
        // injected at the entry pause ([PROF-CPU-COOPERATIVE], OOTB). A
        // runtime that samples itself, such as .NET over EventPipe, attaches
        // as on every other platform.
        await startCooperativeProfileOnLaunch(store, report);
        return;
      }
      // The debuggee PID arrives asynchronously via the DAP `process` event,
      // so wait for it before attaching (avoids a "not ready yet" race).
      report("Waiting for the program to start…");
      const ready = await waitForDebuggeePid(store, debugSessionId);
      if (ready && store.profiler.value.cpu === "starting") {
        report("Attaching the sampler…");
        await handleProfileAttachToDebug(store);
      }
    });
  } finally {
    // Any failure path above leaves the start unfinished — never strand the
    // spinner: only an adopted session reaches "active".
    if (store.profiler.value.cpu !== "active") { store.profilerStopped(); }
  }
}

/** Whether every runtime the server advertises CPU for injects a cooperative sampler. */
function cpuIsCooperativeEverywhere(store: Store): boolean {
  const runtimes = (store.capabilities.value?.runtimes ?? []).filter((runtime) => runtime.cpu !== undefined);
  return runtimes.length > 0 && runtimes.every((runtime) => runtime.cpu?.cooperative === true);
}

/**
 * OOTB CPU profiling for a debug-launched session: inject the in-process
 * sampler at the `stopOnEntry` pause via the courier, resume the program,
 * then adopt the streamed session. No task ports, no elevation prompt.
 */
async function startCooperativeProfileOnLaunch(
  store: Store,
  report: (message: string) => void,
): Promise<void> {
  const client = store.client.value;
  if (client?.isRunning() !== true) { return; }

  report("Waiting for the program to pause at entry…");
  const frameId = await waitForStoppedFrame();
  if (frameId === null) {
    vscode.window.showWarningMessage(
      `${brand()}: The debuggee did not pause at entry, so the profiler could not be injected.`,
    );
    return;
  }

  const sampleRate = vscode.workspace
    .getConfiguration(configSection())
    .get<number>("profiler.sampleRate", DEFAULT_SAMPLE_RATE);

  try {
    const leg1 = await client.sendRequest<CooperativeScript | undefined>(
      "workspace/executeCommand",
      { command: LSP_CMD.cooperativeScript, arguments: [{ sample_rate: sampleRate }] },
    );
    if (leg1?.script === undefined) { return; }

    report("Injecting the in-process sampler…");
    const ack = await evaluateInDebugSession(leg1.script, frameId);
    await vscode.commands.executeCommand("workbench.action.debug.continue");
    if (!ack?.includes(COOPERATIVE_ACK)) {
      vscode.window.showWarningMessage(`${brand()}: Could not inject the in-process CPU sampler.`);
      return;
    }

    report("Starting the sample stream…");
    const raw = await client.sendRequest<StartProfileResult | undefined>(
      "workspace/executeCommand",
      {
        command: LSP_CMD.cooperativeAttach,
        arguments: [{ sample_file: leg1.sample_file, sample_rate: sampleRate }],
      },
    );
    if (raw !== undefined && raw !== null && store.profiler.value.cpu !== "active") {
      const result = toStartedSession(raw);
      adoptSession(store, result, `${brand()}: Profiling current file (PID ${result.pid}, in-process sampler)`);
    }
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    Logger.error(`Cooperative profile start failed: ${msg}`);
    vscode.window.showErrorMessage(`${brand()}: ${msg}`);
  }
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

/**
 * Start profiling a specific PID — the panel's one-click path. Updates the
 * shared session state (status bar, active session) so Stop, Snapshot, and
 * live progress work exactly as they do for the debug-attach flow. The LSP
 * resolves `preset` server-side; `sampleRate`/`includeNative` apply when the
 * preset is `"default"`. Implements [PROF-UI-PROCESSES].
 */
export async function startProfilingForPid(store: Store, pid: number, preset: string): Promise<void> {
  const client = store.client.value;
  if (client?.isRunning() !== true) {
    vscode.window.showWarningMessage(`${brand()}: Language server not running.`);
    return;
  }
  // CPU starts gate on the CPU leg only — a CPU profile may begin while memory
  // tracking is live, but never a second CPU run on top of an active one
  // ([PROF-UI-PROCESSES]).
  if (store.cpuBusy.value) {
    vscode.window.showWarningMessage(busyMessage(store));
    return;
  }

  const cfg = vscode.workspace.getConfiguration(configSection());
  const args: Record<string, unknown> = {
    pid,
    preset,
    sample_rate: cfg.get<number>("profiler.sampleRate", DEFAULT_SAMPLE_RATE),
    include_native: cfg.get<boolean>("profiler.includeNative", false),
  };

  store.profilerStarting();
  try {
    await withUserProgress(cpuStartTitle(), async (report) => {
      report(`Attaching to PID ${pid}…`);
      const raw = await client.sendRequest<StartProfileResult | undefined>("workspace/executeCommand", {
        command: LSP_CMD.start,
        arguments: [args],
      });

      if (raw !== undefined && raw !== null) {
        const result = toStartedSession(raw);
        adoptSession(store, result, `${brand()}: Profiling PID ${result.pid} (${result.runtime} ${result.runtimeVersion})`);
      }
    });
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    Logger.error(`Profile start failed: ${msg}`);
    vscode.window.showErrorMessage(`${brand()}: ${msg}`);
  } finally {
    // A failed start never reached "active" — clear the spinner.
    if (store.profiler.value.cpu !== "active") { store.profilerStopped(); }
  }
}

async function handleProfileStop(store: Store): Promise<void> {
  const client = store.client.value;
  const sessionId = store.profiler.value.cpuSessionId;
  if (client?.isRunning() !== true || sessionId === undefined) {
    vscode.window.showWarningMessage(`${brand()}: No active profiling session.`);
    return;
  }

  try {
    // Collecting samples + writing artifacts takes a beat — show it
    // ([PROF-UI-STATE]).
    const result = await withUserProgress(
      `${brand()}: Stopping profiler`,
      async (report) => {
        report("Collecting results…");
        const report_ = await client.sendRequest<ProfileReport | undefined>("workspace/executeCommand", {
          command: LSP_CMD.stop,
          arguments: [{ session_id: sessionId }],
        });
        return report_ === undefined || report_ === null ? undefined : toProfileResult(report_);
      },
    );

    store.profilerStopped();

    if (result !== undefined && result !== null) {
      lastResult = result;
      applyProfileDecorations(result);
      // Land the user on the self-contained results panel (opened beside so the
      // heat-mapped source stays visible); the raw `.cpuprofile` stays one click
      // away via the completion toast, the panel's button, or "Show Profile
      // Results" ([PROF-CPU-START], #145).
      presentProfileResult(result);
    }
  } catch (err: unknown) {
    store.profilerStopped();
    const msg = err instanceof Error ? err.message : String(err);
    Logger.error(`Profile stop failed: ${msg}`);
    vscode.window.showErrorMessage(`${brand()}: ${msg}`);
  }
}

async function handleProfileSnapshot(store: Store): Promise<void> {
  const client = store.client.value;
  const sessionId = store.profiler.value.cpuSessionId;
  if (client?.isRunning() !== true || sessionId === undefined) {
    vscode.window.showWarningMessage(`${brand()}: No active profiling session.`);
    return;
  }

  try {
    // Snapshots also write the export artifacts \u2014 show the wait like every
    // other profiling flow ([PROF-UI-STATE]).
    const result = await withUserProgress(
      `${brand()}: Taking profile snapshot`,
      async (report) => {
        report("Collecting samples so far\u2026");
        const report_ = await client.sendRequest<ProfileReport | undefined>("workspace/executeCommand", {
          command: LSP_CMD.snapshot,
          arguments: [{ session_id: sessionId }],
        });
        return report_ === undefined || report_ === null ? undefined : toProfileResult(report_);
      },
    );

    if (result !== undefined && result !== null) {
      lastResult = result;
      applyProfileDecorations(result);
      Logger.info(`Profile snapshot: ${result.totalSamples} samples so far`);
      // Fire-and-forget: a toast with a button is sticky and must not block the
      // snapshot handler ([PROF-CPU-START]).
      void vscode.window
        .showInformationMessage(
          `${brand()}: Snapshot \u2014 ${result.totalSamples} samples (profiling continues)`,
          "View Results",
        )
        .then((choice) => {
          if (choice === "View Results") {
            openFlamegraphWebview(result);
          }
        });
    }
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    Logger.error(`Profile snapshot failed: ${msg}`);
    vscode.window.showErrorMessage(`${brand()}: ${msg}`);
  }
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

async function handleProfileAttachToDebug(store: Store): Promise<void> {
  const session = vscode.debug.activeDebugSession;
  if (session === undefined) {
    vscode.window.showWarningMessage(`${brand()}: No active debug session to profile.`);
    return;
  }

  const client = store.client.value;
  if (client?.isRunning() !== true) {
    vscode.window.showWarningMessage(`${brand()}: Language server not running.`);
    return;
  }

  if (store.cpuBusy.value) {
    vscode.window.showWarningMessage(
      `${brand()}: Already profiling (session ${store.profiler.value.cpuSessionId ?? "?"}).`,
    );
    return;
  }

  // Resolve the debuggee's PID captured from the DAP `process` event so we
  // profile the SAME process the debugger is attached to. The LSP profiler is
  // PID-based; the privilege layer handles elevation (macOS helper) transparently.
  const pid = store.getDebuggeeProcessId(session.id);
  if (pid === undefined) {
    vscode.window.showWarningMessage(
      `${brand()}: The debuggee process isn't ready yet — let it start running, then run “Profile Debug Session” again.`,
    );
    return;
  }

  const cfg = vscode.workspace.getConfiguration(configSection());
  const sampleRate = cfg.get<number>("profiler.sampleRate", DEFAULT_SAMPLE_RATE);
  const includeNative = cfg.get<boolean>("profiler.includeNative", false);

  try {
    const raw = await client.sendRequest<StartProfileResult | undefined>("workspace/executeCommand", {
      command: LSP_CMD.start,
      arguments: [{ pid, sample_rate: sampleRate, include_native: includeNative }],
    });

    if (raw !== undefined && raw !== null) {
      const result = toStartedSession(raw);
      adoptSession(store, result, `${brand()}: Profiling debug session (PID ${result.pid})`);
    }
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    Logger.error(`Profile attach-to-debug failed: ${msg}`);
    vscode.window.showErrorMessage(`${brand()}: ${msg}`);
  }
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
