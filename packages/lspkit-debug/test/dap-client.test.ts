import * as assert from "assert";
import {
  continueExecution,
  DapSessionLike,
  evaluate,
  findVariable,
  readFrameVariables,
  scopes,
  stackTrace,
  stepIn,
  stepOut,
  stepOver,
  threads,
  variables,
  waitForStop,
} from "../src/testing/dap-client";

class FakeSession implements DapSessionLike {
  public readonly calls: { command: string; args: unknown }[] = [];
  constructor(private readonly handler: (command: string, args: unknown) => unknown) {}
  customRequest(command: string, args?: unknown): Promise<unknown> {
    this.calls.push({ command, args });
    return Promise.resolve(this.handler(command, args));
  }
}

suite("dap-client request wrappers", () => {
  test("threads returns the thread list, or [] when absent", async () => {
    assert.deepStrictEqual(
      await threads(new FakeSession(() => ({ threads: [{ id: 1, name: "main" }] }))),
      [{ id: 1, name: "main" }],
    );
    assert.deepStrictEqual(await threads(new FakeSession(() => ({}))), []);
  });

  test("stackTrace forwards threadId/levels and returns frames", async () => {
    const session = new FakeSession((_c, args) => ({
      stackFrames: [{ id: 10, name: "main", line: (args as { threadId: number }).threadId, column: 1 }],
    }));
    const stack = await stackTrace(session, 7, 5);
    assert.strictEqual(stack.stackFrames[0].line, 7);
    assert.deepStrictEqual(session.calls[0], {
      command: "stackTrace",
      args: { threadId: 7, startFrame: 0, levels: 5 },
    });
  });

  test("scopes and variables pass references through", async () => {
    const session = new FakeSession((command) =>
      command === "scopes"
        ? { scopes: [{ name: "Locals", variablesReference: 100 }] }
        : { variables: [{ name: "x", value: "1", variablesReference: 0 }] },
    );
    assert.strictEqual((await scopes(session, 10)).scopes[0].variablesReference, 100);
    assert.strictEqual((await variables(session, 100)).variables[0].name, "x");
  });

  test("evaluate defaults to the watch context and honours overrides", async () => {
    const session = new FakeSession((_c, args) => ({
      result: `${(args as { context: string }).context}:${(args as { expression: string }).expression}`,
      variablesReference: 0,
    }));
    assert.strictEqual((await evaluate(session, "x", 10)).result, "watch:x");
    assert.strictEqual((await evaluate(session, "y", 10, "repl")).result, "repl:y");
  });

  test("step/continue wrappers issue the right DAP command", async () => {
    const session = new FakeSession(() => undefined);
    await stepOver(session, 1);
    await stepIn(session, 1);
    await stepOut(session, 1);
    await continueExecution(session, 1);
    assert.deepStrictEqual(
      session.calls.map((c) => c.command),
      ["next", "stepIn", "stepOut", "continue"],
    );
    assert.deepStrictEqual(session.calls[0].args, { threadId: 1 });
  });
});

suite("dap-client variable helpers", () => {
  const session = new FakeSession((command, args) => {
    if (command === "scopes") {
      return {
        scopes: [
          { name: "Locals", variablesReference: 100 },
          { name: "Registers", variablesReference: 0 },
        ],
      };
    }
    if (command === "variables" && (args as { variablesReference: number }).variablesReference === 100) {
      return {
        variables: [
          { name: "x", value: "1", variablesReference: 0 },
          { name: "s", value: '"hi"', variablesReference: 0 },
        ],
      };
    }
    return { variables: [] };
  });

  test("readFrameVariables flattens only referenceable scopes", async () => {
    const vars = await readFrameVariables(session, 10);
    assert.deepStrictEqual(vars.map((v) => v.name), ["x", "s"]);
  });

  test("findVariable finds a named local and returns undefined otherwise", async () => {
    assert.strictEqual((await findVariable(session, 10, "x"))?.value, "1");
    assert.strictEqual(await findVariable(session, 10, "missing"), undefined);
  });
});

suite("waitForStop", () => {
  test("resolves once a thread reports a stack frame", async () => {
    let threadCalls = 0;
    const session = new FakeSession((command) => {
      if (command === "threads") {
        threadCalls += 1;
        return threadCalls === 1 ? { threads: [] } : { threads: [{ id: 1, name: "main" }] };
      }
      return { stackFrames: [{ id: 10, name: "main", line: 2, column: 1, source: { path: "/a" } }] };
    });
    const stop = await waitForStop(session, 5000);
    assert.strictEqual(stop.threadId, 1);
    assert.strictEqual(stop.stack.stackFrames[0].line, 2);
  });

  test("skips a thread whose stackTrace errors and tries the next", async () => {
    const session = new FakeSession((command, args) => {
      if (command === "threads") {
        return { threads: [{ id: 1, name: "a" }, { id: 2, name: "b" }] };
      }
      if ((args as { threadId: number }).threadId === 1) {
        throw new Error("frame unavailable");
      }
      return { stackFrames: [{ id: 20, name: "b", line: 9, column: 1 }] };
    });
    const stop = await waitForStop(session, 5000);
    assert.strictEqual(stop.threadId, 2);
  });

  test("recovers when the threads request itself errors transiently", async () => {
    let calls = 0;
    const session = new FakeSession((command) => {
      if (command === "threads") {
        calls += 1;
        if (calls === 1) {
          throw new Error("not ready");
        }
        return { threads: [{ id: 3, name: "c" }] };
      }
      return { stackFrames: [{ id: 30, name: "c", line: 1, column: 1 }] };
    });
    const stop = await waitForStop(session, 5000);
    assert.strictEqual(stop.threadId, 3);
  });

  test("throws after the timeout when nothing ever stops", async () => {
    const session = new FakeSession((command) =>
      command === "threads" ? { threads: [{ id: 1, name: "main" }] } : { stackFrames: [] },
    );
    await assert.rejects(() => waitForStop(session, 60), /waiting for the debuggee to stop/);
  });
});
