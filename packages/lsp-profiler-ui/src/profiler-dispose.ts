// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
import { disposeProfileDecorations } from "./profiler-decorations";
import { disposeFlamegraphPanel } from "./profiler-flamegraph-html";
import { disposeProfileServer } from "./profile-server";
import { clearLastProfileResult } from "./profiler-results";

/**
 * Dispose the profiler's UI artifacts. Session state lives in the store and is
 * cleared by `store.reset()` on deactivation; the status bar and context keys
 * follow reactively, so there is nothing imperative to tear down here.
 */
export function disposeProfiler(): void {
  disposeProfileDecorations();
  disposeFlamegraphPanel();
  disposeProfileServer();
  clearLastProfileResult();
}
