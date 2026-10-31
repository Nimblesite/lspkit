// Implements [PROF-UI-MEMORY] + [PROF-UI-AUTOPILOT]. See docs/specs/LSP-PROFILER-SPEC.md
import { brand, lspTable } from "./ids";

/** The `<prefix>.lspProfiler.memory.*` LSP command names (one round-trip leg each). */
export const LSP_MEM_CMD = lspTable({
  start: "memory.start",
  snapshot: "memory.snapshot",
  diff: "memory.diff",
  references: "memory.references",
  objectsByType: "memory.objectsByType",
  gcCollect: "memory.gcCollect",
  ingest: "memory.ingest",
  stop: "memory.stop",
});

/** The tracemalloc traceback depth injected at start. */
export const TRACEBACK_DEPTH = 25;

/** The progress title of a memory start; a function so it follows the installed brand. */
export function memStartTitle(): string {
  return `${brand()}: Starting memory tracking`;
}

/** [PROF-UI-STATE] Progress-notification titles, one per memory operation. */
export function memOpTitle(command: string): string {
  switch (command) {
    case LSP_MEM_CMD.snapshot: {
      return `${brand()}: Taking memory snapshot`;
    }
    case LSP_MEM_CMD.diff: {
      return `${brand()}: Comparing memory snapshots`;
    }
    case LSP_MEM_CMD.gcCollect: {
      return `${brand()}: Forcing garbage collection`;
    }
    case LSP_MEM_CMD.references: {
      return `${brand()}: Building the reference graph`;
    }
    default: {
      return `${brand()}: Inspecting memory`;
    }
  }
}
