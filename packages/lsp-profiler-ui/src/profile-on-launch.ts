// Implements [PROF-UI-STATE]. See docs/specs/LSP-PROFILER-SPEC.md
import type * as vscode from "vscode";
import { Logger } from "./logger";
import { shouldProfileOnLaunch } from "./profiler-launch";
import type { ProfilerStore as Store } from "./store";
import { notifyBreakpointsSuppressedForProfiling, startProfilerOnLaunch } from "./profiler-launch-cooperative";

/** "Profile on Launch": profile a debug session that asked for it while the CPU leg is idle. */
export function profileOnLaunch(store: Store, session: vscode.DebugSession): void {
  if (!shouldProfileOnLaunch(session) || store.profiler.value.cpu !== "idle") { return; }
  Logger.info(`Profile on Launch: auto-profiling debug session ${session.id}`);
  notifyBreakpointsSuppressedForProfiling();
  void startProfilerOnLaunch(store, session.id);
}
