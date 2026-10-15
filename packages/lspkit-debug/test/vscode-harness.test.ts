import * as assert from "assert";
import {
  BreakpointSpec,
  DebugSessionHandle,
  makeBreakpointTools,
  makeSessionWaiters,
  VscodeBreakpointApi,
  VscodeSessionApi,
} from "../src/testing/vscode-harness";

function fakeBreakpointApi(): VscodeBreakpointApi & { added: unknown[] } {
  const state = { current: [] as unknown[], added: [] as unknown[] };
  return {
    get breakpoints() {
      return state.current;
    },
    get added() {
      return state.added;
    },
    addBreakpoints(bps: unknown[]) {
      state.current = [...state.current, ...bps];
      state.added.push(...bps);
    },
    removeBreakpoints(bps: readonly unknown[]) {
      const drop = new Set(bps);
      state.current = state.current.filter((b) => !drop.has(b));
    },
  };
}

suite("makeBreakpointTools", () => {
  test("setSourceBreakpoints clears then adds one breakpoint per spec", () => {
    const api = fakeBreakpointApi();
    const made: string[] = [];
    const tools = makeBreakpointTools(api, (file, line, condition) => {
      const bp = `${file}:${line}${condition ? `?${condition}` : ""}`;
      made.push(bp);
      return bp;
    });

    const specs: BreakpointSpec[] = [3, { line: 7, condition: "n == 3" }, { line: 9 }];
    tools.setSourceBreakpoints("/src/a.osp", specs);

    assert.deepStrictEqual(made, ["/src/a.osp:3", "/src/a.osp:7?n == 3", "/src/a.osp:9"]);
    assert.deepStrictEqual(api.breakpoints, made);
  });

  test("setSourceBreakpoints replaces a previous set rather than appending", () => {
    const api = fakeBreakpointApi();
    const tools = makeBreakpointTools(api, (file, line) => `${file}:${line}`);
    tools.setSourceBreakpoints("/a.osp", [1]);
    tools.setSourceBreakpoints("/a.osp", [2]);
    assert.deepStrictEqual(api.breakpoints, ["/a.osp:2"]);
  });

  test("clearBreakpoints removes everything", () => {
    const api = fakeBreakpointApi();
    const tools = makeBreakpointTools(api, (file, line) => `${file}:${line}`);
    tools.setSourceBreakpoints("/a.osp", [1, 2]);
    tools.clearBreakpoints();
    assert.deepStrictEqual(api.breakpoints, []);
  });
});

suite("makeSessionWaiters.waitForDebugSessionStart", () => {
  test("resolves with the next started session and disposes the listener", async () => {
    let listener: ((s: DebugSessionHandle) => void) | undefined;
    let disposed = false;
    const api: VscodeSessionApi = {
      onDidStartDebugSession(l) {
        listener = l;
        return { dispose() { disposed = true; } };
      },
      activeDebugSession: undefined,
    };
    const waiters = makeSessionWaiters(api);
    const started = waiters.waitForDebugSessionStart(2000);
    listener?.({ id: "session-1" });
    assert.strictEqual((await started).id, "session-1");
    assert.ok(disposed, "the start listener is disposed");
  });

  test("rejects (and disposes) when no session starts before the timeout", async () => {
    let disposed = false;
    const api: VscodeSessionApi = {
      onDidStartDebugSession() {
        return { dispose() { disposed = true; } };
      },
      activeDebugSession: undefined,
    };
    const waiters = makeSessionWaiters(api);
    await assert.rejects(() => waiters.waitForDebugSessionStart(30), /did not start within 30ms/);
    assert.ok(disposed, "the listener is disposed on timeout");
  });
});

suite("makeSessionWaiters.waitForDebugSessionEnd", () => {
  function apiWithActive(getActive: () => DebugSessionHandle | undefined): VscodeSessionApi {
    return {
      onDidStartDebugSession() {
        return { dispose() {} };
      },
      get activeDebugSession() {
        return getActive();
      },
    };
  }

  test("resolves once the active session clears", async () => {
    let active: DebugSessionHandle | undefined = { id: "s1" };
    const waiters = makeSessionWaiters(apiWithActive(() => active));
    const ended = waiters.waitForDebugSessionEnd(2000, "s1");
    setTimeout(() => { active = undefined; }, 50);
    await ended;
  });

  test("resolves immediately when a different session is active", async () => {
    const waiters = makeSessionWaiters(apiWithActive(() => ({ id: "other" })));
    await waiters.waitForDebugSessionEnd(2000, "s1");
  });

  test("throws when the matching session never ends", async () => {
    const waiters = makeSessionWaiters(apiWithActive(() => ({ id: "s1" })));
    await assert.rejects(() => waiters.waitForDebugSessionEnd(40, "s1"), /did not end within 40ms/);
  });

  test("resolves when no session is active and no id filter is given", async () => {
    const waiters = makeSessionWaiters(apiWithActive(() => undefined));
    await waiters.waitForDebugSessionEnd(2000);
  });
});
