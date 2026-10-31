// Implements [PROF-UI-STATE], [PROF-CPU-COOPERATIVE]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Profile-on-launch and the cooperative in-process sampler injection.
 */

import * as vscode from "vscode";
import { brand } from "./ids";
import type { ProfilerStore as Store } from "./store";
import { evaluateInDebugSession, waitForStoppedFrame } from "./dap-evaluate";
import { reportFailure } from "./failure";
import { withUserProgress } from "./progress-ops";
import { waitForDebuggeePid } from "./profiler-launch";
import { executeServerCommand } from "./server-command";
import type { CooperativeScript, StartProfileResult } from "./wire-generated";
import { CPU_CMD, adoptSession, attachToDebuggee, cpuStartTitle, samplerSettings, settleStart } from "./profiler-session-core";

/** Ack printed by the injected cooperative sampler ([PROF-CPU-COOPERATIVE]). */
const COOPERATIVE_ACK = "__LSPPROF_CPU_ACK__";

/** The pieces one cooperative injection needs, kept as one parameter. */
interface CooperativeInject {
  readonly client: NonNullable<Store["client"]["value"]>;
  readonly frameId: number;
  readonly report: (message: string) => void;
}

/** A fetched cooperative script plus the sample rate it was generated for. */
interface CooperativeScriptRequest {
  readonly script: CooperativeScript;
  readonly sampleRate: number;
}

/** Shown once per session: a profiling launch neutralises breakpoints (ux-6). */
let breakpointSuppressionNoticeShown = false;

/**
 * Tell the user, once, that a profiling launch runs to completion with their
 * breakpoints disabled, or a Run & Profile silently never stops at a breakpoint
 * the gutter still shows armed (ux-6). Only fires when breakpoints are set.
 */
export function notifyBreakpointsSuppressedForProfiling(): void {
  if (breakpointSuppressionNoticeShown || vscode.debug.breakpoints.length === 0) {
    return;
  }
  breakpointSuppressionNoticeShown = true;
  void vscode.window.showInformationMessage(
    `${brand()}: Profiling run — your breakpoints are disabled so the program runs to completion. Launch without profiling to debug with breakpoints.`,
  );
}

/**
 * Auto-start for a freshly launched debug session under one progress
 * notification with stage messages; the status bar spins until the live sample
 * counter takes over, so a click is never followed by silence ([PROF-UI-STATE]).
 */
export async function startProfilerOnLaunch(store: Store, debugSessionId: string): Promise<void> {
  store.profilerStarting();
  try {
    await withUserProgress(cpuStartTitle(), async (report) => {
      // On macOS, task ports are gated behind entitled debuggers, so a runtime that can
      // only be sampled from outside is sampled by its cooperative in-process
      // sampler, injected at the entry pause ([PROF-CPU-COOPERATIVE]). A runtime
      // that samples itself, such as .NET over EventPipe, attaches as anywhere.
      if (process.platform === "darwin" && cpuIsCooperativeEverywhere(store)) {
        await startCooperativeProfileOnLaunch(store, report);
        return;
      }
      await attachOnLaunch(store, debugSessionId, report);
    });
  } finally {
    settleStart(store);
  }
}

/** Wait for the debuggee's PID, which arrives with the DAP `process` event, then attach to it. */
async function attachOnLaunch(store: Store, debugSessionId: string, report: (message: string) => void): Promise<void> {
  report("Waiting for the program to start…");
  const ready = await waitForDebuggeePid(store, debugSessionId);
  const pid = store.getDebuggeeProcessId(debugSessionId);
  const client = store.client.value;
  const starting = (): boolean => store.profiler.value.cpu === "starting";
  if (ready && pid !== undefined && client?.isRunning() === true && starting()) {
    report("Attaching the sampler…");
    await attachToDebuggee(store, client, { pid, launching: starting });
  }
}

/** Whether every runtime the server advertises CPU for injects a cooperative sampler. */
function cpuIsCooperativeEverywhere(store: Store): boolean {
  const runtimes = (store.capabilities.value?.runtimes ?? []).filter((runtime) => runtime.cpu !== undefined);
  return runtimes.length > 0 && runtimes.every((runtime) => runtime.cpu?.cooperative === true);
}

/**
 * CPU profiling for a debug-launched session with no task ports and no
 * elevation prompt: inject the in-process sampler at the `stopOnEntry` pause.
 */
async function startCooperativeProfileOnLaunch(store: Store, report: (message: string) => void): Promise<void> {
  const client = store.client.value;
  if (client?.isRunning() !== true) { return; }
  report("Waiting for the program to pause at entry…");
  const frameId = await waitForStoppedFrame();
  if (frameId === null) {
    warnCooperativeEntryMissing();
    return;
  }
  await injectCooperativeSamplerSafely(store, { client, frameId, report });
}

/** Tell the user why the sampler could not be injected. */
function warnCooperativeEntryMissing(): void {
  vscode.window.showWarningMessage(
    `${brand()}: The debuggee did not pause at entry, so the profiler could not be injected.`,
  );
}

/** Run the injection and turn a failure into the shared failure report. */
async function injectCooperativeSamplerSafely(store: Store, context: CooperativeInject): Promise<void> {
  try {
    await injectCooperativeSampler(store, context);
  } catch (error: unknown) {
    reportFailure("Cooperative profile start", error);
  }
}

/** Evaluate the sampler script at the entry pause, resume, then adopt its sample stream. */
async function injectCooperativeSampler(store: Store, context: CooperativeInject): Promise<void> {
  const requested = await requestCooperativeScript(context);
  if (requested === undefined) { return; }
  context.report("Injecting the in-process sampler…");
  const ack = await evaluateInDebugSession(requested.script.script, context.frameId);
  await vscode.commands.executeCommand("workbench.action.debug.continue");
  if (ack?.includes(COOPERATIVE_ACK) !== true) {
    vscode.window.showWarningMessage(`${brand()}: Could not inject the in-process CPU sampler.`);
    return;
  }
  await startCooperativeStream(store, context, requested);
}

/** Ask the server for the injection script, or undefined when it answered without one. */
async function requestCooperativeScript(context: CooperativeInject): Promise<CooperativeScriptRequest | undefined> {
  const sampleRate = samplerSettings().sample_rate;
  const script = await executeServerCommand<CooperativeScript>(context.client, CPU_CMD.cooperativeScript, { sample_rate: sampleRate });
  return script?.script === undefined ? undefined : { script, sampleRate };
}

/** Attach the in-process sample stream and adopt the session it starts. */
async function startCooperativeStream(store: Store, context: CooperativeInject, requested: CooperativeScriptRequest): Promise<void> {
  context.report("Starting the sample stream…");
  const raw = await executeServerCommand<StartProfileResult>(context.client, CPU_CMD.cooperativeAttach,
    { sample_file: requested.script.sample_file, sample_rate: requested.sampleRate });
  if (store.profiler.value.cpu === "active") { return; }
  adoptSession(store, raw, (result) => `${brand()}: Profiling current file (PID ${String(result.pid)}, in-process sampler)`);
}
