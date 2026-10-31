// Implements [PROF-UI-MEMORY] + [PROF-UI-AUTOPILOT]. See docs/specs/LSP-PROFILER-SPEC.md
import * as fs from "node:fs";
import { executeServerCommand } from "./server-command";
import { toMemoryIngestResult } from "./wire-adapters";
import type { MemoryResult } from "./wire-generated";
import type { MemoryIngestResult } from "./memory-dashboard-mapping";
import { asNumber } from "./memory-dashboard-mapping";
import { LSP_MEM_CMD } from "./memory-capture-ids";
import type { LspClient } from "./memory-capture-types";
import { delay } from "./timeouts";

/** [PROF-PY-MEMORY-FINAL] How long to wait for the at-exit snapshot file after the
 *  program exits (covers the terminate-event/final-flush race), and the poll
 *  cadence while waiting. */
const FINAL_SNAPSHOT_WAIT_MS = 3000;
const FINAL_SNAPSHOT_POLL_MS = 100;

/** Post a script's raw output to `<prefix>.lspProfiler.memory.ingest` and read the structured result. */
export async function ingestOutput(client: LspClient, memorySessionId: string, output: string): Promise<MemoryIngestResult | null> {
  const result = await executeServerCommand<MemoryResult>(client, LSP_MEM_CMD.ingest, { memory_session_id: memorySessionId, output });
  return result === undefined ? null : toMemoryIngestResult(result);
}

/**
 * Read the debuggee's at-exit snapshot payload, deleting the file once it is read
 * intact ([PROF-PY-MEMORY-FINAL]). The debuggee writes atomically (sibling temp +
 * `os.replace`), so a readable marker-bearing file is whole. A short poll covers
 * the terminate-event/flush race. The file is unlinked ONLY once a complete
 * (marker-bearing) payload is read — a missing or marker-less read is never
 * destructive. Returns null when no usable payload arrived by the deadline.
 */
export async function readFinalSnapshot(path: string): Promise<string | null> {
  return readFinalSnapshotUntil(path, Date.now() + FINAL_SNAPSHOT_WAIT_MS);
}

/** Poll for the at-exit snapshot until it lands or the deadline passes. */
async function readFinalSnapshotUntil(path: string, deadline: number): Promise<string | null> {
  const contents = await fs.promises.readFile(path, "utf8").catch(() => null);
  if (contents?.includes("__LSPPROF_MEM__") === true) {
    await fs.promises.unlink(path).catch(() => null);
    return contents;
  }
  if (Date.now() >= deadline) { return null; }
  await delay(FINAL_SNAPSHOT_POLL_MS);
  return readFinalSnapshotUntil(path, deadline);
}

/** Read a `currentMemory` figure off an ingest result (for logging/summaries). */
export function snapshotCurrentMemory(result: MemoryIngestResult): number {
  return asNumber(result.currentMemory);
}
