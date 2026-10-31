// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Pure helpers for the "profile on launch" flow: deciding whether a freshly
 * started debug session should be auto-profiled, and waiting for the
 * debuggee's PID to land. Stateless — the session flow stays in profiler.ts.
 */

import * as vscode from "vscode";
import { configSection, debugType } from "./ids";
import type { ProfilerStore as Store } from "./store";
import { POLL_INTERVAL_MS, STARTUP_TIMEOUT_MS, delay } from "./timeouts";
import { numberField } from "./unknown-shape";

/** [PROF-ERRORS] `UNSUPPORTED_RUNTIME`: no runtime recognises the PID (yet). */
const UNSUPPORTED_RUNTIME = -32_002;
/** Gap between start attempts while a launched program's runtime comes up. */
const RETRY_GAP_MS = 100;

/**
 * Whether a freshly started debug session should be auto-profiled: either its
 * launch configuration asked for it (`profileOnLaunch: true`, set by the
 * metric-explicit "Run & Profile CPU (Current File)" entry point — #82) or the
 * user enabled the global `<section>.profiler.profileOnLaunch` setting.
 */
export function shouldProfileOnLaunch(
  session: Pick<vscode.DebugSession, "type" | "configuration">,
): boolean {
  if (session.type !== debugType()) { return false; }
  // A "Run & Track Memory" launch tracks memory, not CPU. Never auto-start the
  // CPU sampler on it — even when the global setting is on (read directly
  // below, so the stamp carve-out in applyDebugConfigDefaults is not enough) —
  // or the cooperative sampler and tracemalloc collide on the single entry
  // pause (dap-1).
  if (session.configuration.memoryTrackOnLaunch === true) { return false; }
  if (session.configuration.profileOnLaunch === true) { return true; }
  return vscode.workspace
    .getConfiguration(configSection())
    .get<boolean>("profiler.profileOnLaunch", false);
}

/**
 * Resolve once the debuggee's PID for `sessionId` is known (captured from the
 * DAP `process` event into the store), or after a bounded wait. Returns whether
 * the PID became available. Used by the "Profile on Launch" auto-attach so it
 * doesn't fire before debugpy reports the process.
 */
export async function waitForDebuggeePid(store: Store, sessionId: string): Promise<boolean> {
  return waitForDebuggeePidUntil(store, sessionId, Date.now() + STARTUP_TIMEOUT_MS);
}

/** Poll the store for a debuggee PID until it lands or the deadline passes. */
async function waitForDebuggeePidUntil(store: Store, sessionId: string, deadline: number): Promise<boolean> {
  if (store.getDebuggeeProcessId(sessionId) !== undefined) {
    return true;
  }
  if (Date.now() >= deadline) {
    return false;
  }
  await delay(POLL_INTERVAL_MS);
  return waitForDebuggeePidUntil(store, sessionId, deadline);
}

/**
 * Send a CPU start for a program the debugger just launched. The adapter
 * reports the PID before the program's runtime has opened its diagnostics
 * endpoint, so while `pending()` holds and for up to the startup timeout an
 * `UNSUPPORTED_RUNTIME` refusal means "not yet" and the start is sent again
 * ([PROF-UI-PROCESSES]). Any other error, or the same one later, is final.
 */
export async function startWhenRuntimeReady<Value>(send: () => Promise<Value>, pending: () => boolean): Promise<Value> {
  const deadline = Date.now() + STARTUP_TIMEOUT_MS;
  return startWhenRuntimeReadyBy(deadline, send, pending);
}

/** One retry step: try the start; on "not yet", wait and try again. */
async function startWhenRuntimeReadyBy<Value>(deadline: number, send: () => Promise<Value>, pending: () => boolean): Promise<Value> {
  try {
    return await send();
  } catch (error: unknown) {
    const notYet = numberField(error, "code") === UNSUPPORTED_RUNTIME && pending() && Date.now() < deadline;
    if (!notYet) { throw error; }
    await delay(RETRY_GAP_MS);
    return startWhenRuntimeReadyBy(deadline, send, pending);
  }
}
