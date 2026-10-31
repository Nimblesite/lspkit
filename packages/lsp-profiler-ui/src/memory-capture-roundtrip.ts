// Implements [PROF-UI-MEMORY] + [PROF-UI-AUTOPILOT]. See docs/specs/LSP-PROFILER-SPEC.md
import { brand } from "./ids";
import { Logger } from "./logger";
import { errorText } from "./failure";
import { executeServerCommand } from "./server-command";
import type { ProfilerStore as Store } from "./store";
import { isDebuggeePaused } from "./debuggee-pause";
import { beginMemoryOp, endMemoryOp, isDirectMemorySession } from "./memory-capture-state";
import type { MemoryOperation } from "./memory-capture-types";
import { resolveActiveSession, warnUnlessQuiet } from "./memory-capture-types";
import { LSP_MEM_CMD } from "./memory-capture-ids";
import { requestStep } from "./memory-capture-legs";

/**
 * End the tracked session on the server ([PROF-MEMORY-SESSION]): it answers no
 * further step and its heap graphs are deleted ([PROF-DOTNET-MEMORY] `stop`).
 * Call it while the store still holds the session id; a failure is logged,
 * because the UI's own tracking ends either way.
 */
export async function endServerSession(store: Store): Promise<void> {
  const client = store.client.value;
  const { memorySessionId } = store.profiler.value;
  if (client?.isRunning() !== true || memorySessionId === undefined) { return; }
  try {
    await executeServerCommand<unknown>(client, LSP_MEM_CMD.stop, { memory_session_id: memorySessionId });
  } catch (error: unknown) {
    Logger.warn(`[Memory] ${LSP_MEM_CMD.stop} failed: ${errorText(error)}`);
  }
}

/**
 * Run one memory operation's round-trip and return the LSP's structured result.
 *
 * Returns null (with an actionable message unless `quiet`) when there is no
 * session, the debuggee cannot be paused, or evaluation fails.
 */
export async function runMemoryOperation(op: MemoryOperation): Promise<ReturnType<typeof requestStep>> {
  const active = resolveActiveSession(op);
  if (active === null || refusedWhilePaused(op)) { return null; }
  beginMemoryOp();
  try {
    return await requestStep(op, active);
  } catch (error: unknown) {
    return handleMemoryOperationError(op, error);
  } finally {
    endMemoryOp();
  }
}

/** Log and surface one failed memory round-trip, always resolving to null. */
function handleMemoryOperationError(op: MemoryOperation, error: unknown): null {
  const msg = errorText(error);
  Logger.warn(`[Memory] ${op.command} round-trip failed: ${msg}`);
  warnUnlessQuiet(op, `${brand()}: ${msg}`);
  return null;
}

/**
 * A direct runtime reads the live heap, which it cannot do while the debugger
 * holds the program paused: the step would wait for a resume that may never
 * come ([PROF-MEMORY-MODE]). Refuse it and say why.
 */
function refusedWhilePaused(op: MemoryOperation): boolean {
  if (!isDirectMemorySession() || !isDebuggeePaused(op.store.profiler.value.memoryDebugSessionId)) { return false; }
  warnUnlessQuiet(op, `${brand()}: Continue the program to read its memory — its heap cannot be read while the debugger holds it paused.`);
  return true;
}
