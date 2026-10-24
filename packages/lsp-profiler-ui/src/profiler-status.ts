// Implements [PROF-UI-STATE] + [PROF-UI-STATE].
// See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Profiler status-bar lifecycle: hidden → starting spinner → live flame
 * counter. A CPU-profiling start is never silent — between the click and the
 * first sample batch the item shows `$(loading~spin)`, then the LSP's progress
 * notifications drive the live counter.
 *
 * The item renders purely from the store's reactive `profiler` signal
 * ([PROF-UI-PROCESSES]): one `effect` repaints it whenever the session
 * state changes, so there is no imperative "set status" call to forget. Owned
 * here so profiler.ts stays focused on session flow.
 */

import * as vscode from "vscode";
import { cmd, brand } from "./ids";
import { METHODS, type ProfileProgress } from "./wire-generated";
import { effect } from "@preact/signals-core";
import type { LanguageClient } from "vscode-languageclient/node";
import type { ProfilerStore as Store } from "./store";
import type { ProfilerActivity, ProfilerSession } from "./profiler-state";
import { formatProfileDuration, formatSampleCount } from "./profiler-format";

/** LSP notification carrying live profiling progress. */
const PROFILER_PROGRESS_NOTIFICATION = METHODS.CPU_PROGRESS;

/** Status bar priority — slightly lower than the host's main item. */
const PROFILER_STATUS_BAR_PRIORITY = 99;

let statusBarItem: vscode.StatusBarItem | undefined;
/** Last-rendered CPU activity, so the e2e seam knows when the item is hidden. */
let renderedActivity: ProfilerActivity = "idle";

/**
 * Create the profiler status-bar item (click = stop) and bind it to the store's
 * `profiler` signal. Returns a disposable that tears down both the effect and
 * the item.
 */
export function bindProfilerStatusBar(store: Store): vscode.Disposable {
  const item = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    PROFILER_STATUS_BAR_PRIORITY,
  );
  item.command = cmd("profileStop");
  statusBarItem = item;
  renderStatusBar(item, store.profiler.value);
  const disposeEffect = effect(() => { renderStatusBar(item, store.profiler.value); });
  return {
    dispose() {
      disposeEffect();
      item.dispose();
      statusBarItem = undefined;
      renderedActivity = "idle";
    },
  };
}

/** Repaint the item to match the current CPU session state. */
function renderStatusBar(item: vscode.StatusBarItem, session: ProfilerSession): void {
  renderedActivity = session.cpu;
  if (session.cpu === "idle") {
    item.hide();
    return;
  }
  if (session.cpu === "starting") {
    // [PROF-UI-STATE] Click → spinner, never click → silence.
    item.text = "$(loading~spin) Profiler starting…";
    item.tooltip = `${brand()} is setting up CPU profiling for the program`;
    item.backgroundColor = undefined;
    item.show();
    return;
  }

  renderActive(item, session);
  item.show();
}

/** Active: the live flame counter, warning-tinted so it reads as "running". */
function renderActive(item: vscode.StatusBarItem, session: ProfilerSession): void {
  item.backgroundColor = new vscode.ThemeColor("statusBarItem.warningBackground");
  if (session.sampleCount === 0) {
    item.text = "$(flame) Profiling...";
    item.tooltip = `Profiling PID ${String(session.cpuPid ?? "?")} — click to stop`;
    return;
  }
  const samples = formatSampleCount(session.sampleCount);
  const duration = formatProfileDuration(session.durationSecs);
  item.text = `$(flame) ${samples} samples (${duration})`;
  item.tooltip = `PID ${String(session.cpuPid ?? "?")} — ${samples} samples, ${duration}\nTop: ${session.topFunction ?? "—"}\nClick to stop`;
}

/**
 * The profiler status-bar text, or undefined when idle/hidden. E2e seam for
 * [PROF-UI-STATE]/[PROF-UI-STATE]: the lifecycle
 * (starting spinner → live samples) lands here and StatusBarItem state is not
 * readable via the public API.
 */
export function profilerStatusText(): string | undefined {
  if (renderedActivity === "idle") { return undefined; }
  return statusBarItem?.text;
}

/**
 * Bind the `lspProfiler/cpu/progress` handler to whichever LanguageClient the
 * store currently holds — reactively, so the live sample counter never goes
 * silently dead. The runtime can be re-created within one session (store reset
 * → a brand-new client), and a slow cold start can outlast any fixed timeout,
 * so a one-shot registration is wrong ([PROF-UI-STATE]).
 *
 * `onNotification` may be called before the client connects (the handler is
 * queued), and vscode-languageclient replays it across restarts of the same
 * instance — so registering once per client *instance* is sufficient.
 */
export function registerProgressListener(store: Store): vscode.Disposable {
  let boundClient: LanguageClient | undefined;
  let registration: vscode.Disposable | undefined;

  const disposeEffect = effect(() => {
    const client = store.client.value;
    if (client === undefined || client === boundClient) { return; }
    registration?.dispose();
    boundClient = client;
    registration = onProgress(store, client);
  });
  return { dispose() { disposeEffect(); registration?.dispose(); registration = undefined; boundClient = undefined; } };
}

/** Feed `client`'s progress for the active CPU session into the store. */
function onProgress(store: Store, client: LanguageClient): vscode.Disposable {
  return client.onNotification(PROFILER_PROGRESS_NOTIFICATION, (params: ProfileProgress) => {
    if (params.session_id === store.profiler.value.cpuSessionId) {
      store.profilerProgress(params.sample_count, params.duration_secs, params.top_function ?? "");
    }
  });
}
