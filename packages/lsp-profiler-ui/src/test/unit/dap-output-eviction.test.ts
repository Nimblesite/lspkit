// Tests [PROF-UI-LIFETIME]: per-session DAP state leaves with its session.
// Plain mocha: `dap-output` imports no `vscode`; the debug API is a fake.
import * as assert from "node:assert";
import type { DebugSessionEnds } from "../../dap-output";
import {
  appendDebugOutput,
  debugSessionStateCount,
  registerDebugOutputEviction,
  trackResumeRequest,
  trackSuspensionEvent,
} from "../../dap-output";

/** Debug sessions a long editor session runs and ends. */
const SESSIONS = 16;
/** Output one session prints. */
const OUTPUT = "x".repeat(1024);

/** A debug API whose sessions the test ends by hand. */
function fakeDebug(): DebugSessionEnds & { end: (id: string) => void } {
  const listeners: ((session: { readonly id: string }) => void)[] = [];
  return {
    onDidTerminateDebugSession(listener) {
      listeners.push(listener);
      return { dispose: () => { listeners.splice(listeners.indexOf(listener), 1); } };
    },
    end(id) {
      for (const listener of listeners) { listener({ id }); }
    },
  };
}

/** Give a session output, a stopped thread and an in-flight resume. */
function exercise(id: string): void {
  appendDebugOutput(id, OUTPUT);
  trackSuspensionEvent(id, "stopped", { threadId: 1 });
  trackResumeRequest(id, { type: "request", seq: 7, command: "next", arguments: { threadId: 1 } });
}

suite("DAP session state leaves with its session", () => {
  test("ended sessions retain no output, stop-state or pending resumes", () => {
    const debug = fakeDebug();
    const eviction = registerDebugOutputEviction(debug);
    const before = debugSessionStateCount();
    for (let index = 0; index < SESSIONS; index += 1) {
      const id = `ended-${String(index)}`;
      exercise(id);
      assert.ok(debugSessionStateCount() > before, "the live session is tracked");
      debug.end(id);
    }
    eviction.dispose();
    assert.strictEqual(
      debugSessionStateCount(),
      before,
      `${String(debugSessionStateCount() - before)} ended debug sessions still hold their state`,
    );
  });
});
