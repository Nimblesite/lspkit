// Implements [PROF-UI-MEMORY] + [PROF-UI-AUTOPILOT]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The editor-as-courier memory round-trip and result presentation.
 *
 * The capture engine is split into focused siblings (command ids, shared state,
 * the two round-trip legs, orchestration and presentation) and re-exported here
 * so callers keep one import site. Both the manual commands and the autopilot
 * drive memory through the same primitives, so an auto-capture is
 * byte-for-byte the same flow as a hand-clicked one.
 */

import { withUserProgress } from "./progress-ops";
import type { ProfilerStore as Store } from "./store";
import type { MemoryIngestResult } from "./memory-dashboard-mapping";
import { memOpTitle } from "./memory-capture-ids";
import { runMemoryOperation } from "./memory-capture-roundtrip";

export { LSP_MEM_CMD, TRACEBACK_DEPTH, memStartTitle } from "./memory-capture-ids";
export { isMemoryOperationInFlight, lastMemorySnapshot, hasCapturedSnapshot, resetCaptureState, setDirectMemorySession, isDirectMemorySession } from "./memory-capture-state";
export { ingestOutput, readFinalSnapshot, snapshotCurrentMemory } from "./memory-capture-ingest";
export { endServerSession } from "./memory-capture-roundtrip";
export { captureSnapshotAndDiff, presentDiff, presentSnapshot } from "./memory-capture-present";
export type { CaptureResult } from "./memory-capture-present";

/**
 * Run one memory operation under a user-facing progress notification (the manual
 * command path: snapshot / diff / gc / references). The pause → evaluate → analyze
 * round-trip takes a beat, so its stages narrate under one notification
 * ([PROF-UI-STATE]).
 */
export async function runMemoryScript(
  store: Store,
  command: string,
  extraArgs: Record<string, unknown> = {},
): Promise<MemoryIngestResult | null> {
  return withUserProgress(
    memOpTitle(command),
    async (report) => {
      const result = await runMemoryOperation({ store, command, extraArgs, report, quiet: false });
      return result;
    },
  );
}
