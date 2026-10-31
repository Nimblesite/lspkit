// Implements [PROF-UI-MEMORY]. See docs/specs/LSP-PROFILER-SPEC.md
import { brand } from "./ids";
import { acquireStoppedFrame, evaluateInDebugSession } from "./dap-evaluate";
import { executeServerCommand } from "./server-command";
import { toMemoryIngestResult } from "./wire-adapters";
import type { MemoryStep } from "./wire-generated";
import type { MemoryIngestResult } from "./memory-dashboard-mapping";
import type { ActiveMemorySession, MemoryOperation, OperationContext } from "./memory-capture-types";
import { warnUnlessQuiet } from "./memory-capture-types";
import { ingestOutput } from "./memory-capture-ingest";

/** Leg 1: ask for the operation's step; a completed step is the result, a courier step runs in the debuggee. */
export async function requestStep(op: MemoryOperation, active: ActiveMemorySession): Promise<MemoryIngestResult | null> {
  op.report("Asking the profiler…");
  const step = await executeServerCommand<MemoryStep>(active.client, op.command,
    { memory_session_id: active.memorySessionId, ...op.extraArgs });
  if (step === undefined) { return null; }
  if (step.kind === "Completed") { return toMemoryIngestResult(step.result); }
  return courierStep(op, active, step.script);
}

/**
 * A courier step ([PROF-MEMORY-MODE]): the runtime cannot read its own heap,
 * so the script runs inside the paused debuggee and its output is posted back.
 */
async function courierStep(op: MemoryOperation, active: ActiveMemorySession, script: string): Promise<MemoryIngestResult | null> {
  op.report("Pausing the program…");
  const acquired = await acquireStoppedFrame();
  if (acquired === null) {
    warnUnlessQuiet(op, `${brand()}: Could not pause the program for memory inspection — pause at a breakpoint and retry.`);
    return null;
  }
  try {
    return await evaluateAndIngest(op, { ...active, frameId: acquired.frameId }, script);
  } finally {
    await acquired.release();
  }
}

/** Leg 2: run the script in the frame, then post the output to ingest. */
async function evaluateAndIngest(op: MemoryOperation, ctx: OperationContext, script: string): Promise<MemoryIngestResult | null> {
  if (script === "") { return null; }
  op.report("Inspecting the debuggee…");
  const output = await evaluateInDebugSession(script, ctx.frameId);
  if (output === null) {
    warnUnlessQuiet(op, `${brand()}: Could not run the memory script in the debuggee.`);
    return null;
  }
  op.report("Analyzing…");
  return ingestOutput(ctx.client, ctx.memorySessionId, output);
}
