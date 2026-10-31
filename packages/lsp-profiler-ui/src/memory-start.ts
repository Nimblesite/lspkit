// Implements [PROF-UI-MEMORY], [PROF-UI-PROCESSES] (memory leg). See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Starting memory tracking: from the menu, from a Processes row and from a
 * "Run & Track Memory (Current File)" launch (#82).
 */

export { handleMemoryStart } from "./memory-start-core";
export { trackMemoryOnLaunch } from "./memory-launch";
