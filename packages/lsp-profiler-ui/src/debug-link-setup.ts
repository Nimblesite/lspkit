// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
import type * as vscode from "vscode";
import { registerDebugProfilerLink } from "./debug-link";
import { walkReferences } from "./memory-ref-picker";
import type { ProfilerStore } from "./store";

/** A debugger variable's type retainers, from the memory session tracking that same debug session. */
function typeRetainers(store: ProfilerStore): (type: string, sessionId: string) => Promise<void> {
  return async (type, sessionId) => {
    if (store.profiler.value.memoryDebugSessionId !== sessionId) { throw new Error("Track this debug session before inspecting type retainers."); }
    await walkReferences(store, type);
  };
}

/** Register the debugger "inspect type" link against the profiler store. */
export function registerDebugProfilerSetup(store: ProfilerStore, debugType: string): vscode.Disposable[] {
  return registerDebugProfilerLink({ debugType, inspectType: typeRetainers(store) });
}
