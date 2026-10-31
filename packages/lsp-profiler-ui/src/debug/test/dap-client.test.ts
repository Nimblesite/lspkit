import * as assert from "node:assert";
import {
  continueExecution,
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
} from "../testing/dap-client";
import type { DapSessionLike } from "../testing/dap-client";

/** True when `value` is a non-null object whose fields can be inspected. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** A numeric request argument (thread id, variables reference), or undefined. */
function numberArg(args: unknown, key: string): number | undefined {
  if (!isRecord(args)) {
    return undefined;
  }
  const value = args[key];
  return typeof value === "number" ? value : undefined;
}

/** A string request argument (expression, context), or undefined. */
function stringArg(args: unknown, key: string): string | undefined {
  if (!isRecord(args)) {
    return undefined;
  }
  const value = args[key];
  return typeof value === "string" ? value : undefined;
}

/** A fake handler that ignores every request and answers with nothing. */
function noResponse(): undefined {
  return undefined;
}

class FakeSession implements DapSessionLike {
  public readonly calls: { command: string; args: unknown }[] = [];
  private readonly handler: (command: string, args: unknown) => unknown;

  public constructor(handler: (command: string, args: unknown) => unknown) {
    this.handler = handler;
  }

  public async customRequest(command: string, args?: unknown): Promise<unknown> {
    this.calls.push({ command, args });
    return this.handler(command, args);
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
    const session = new FakeSession((_command, args) => ({
      stackFrames: [
        { id: 10, name: "main", line: numberArg(args, "threadId") ?? 0, column: 1 },
      ],
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
    const frameScopes = await scopes(session, 10);
    assert.strictEqual(frameScopes.scopes[0].variablesReference, 100);
    const group = await variables(session, 100);
    assert.strictEqual(group.variables[0].name, "x");
  });

  test("evaluate defaults to the watch context and honours overrides", async () => {
    const session = new FakeSession((_command, args) => ({
      result: `${stringArg(args, "context")}:${stringArg(args, "expression")}`,
      variablesReference: 0,
    }));
    const watch = await evaluate(session, { expression: "x", frameId: 10 });
    assert.strictEqual(watch.result, "watch:x");
    const repl = await evaluate(session, { expression: "y", frameId: 10, context: "repl" });
    assert.strictEqual(repl.result, "repl:y");
  });

  test("step/continue wrappers issue the right DAP command", async () => {
    const session = new FakeSession(noResponse);
    await stepOver(session, 1);
    await stepIn(session, 1);
    await stepOut(session, 1);
    await continueExecution(session, 1);
    assert.deepStrictEqual(
      session.calls.map((call) => call.command),
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
    if (command === "variables" && numberArg(args, "variablesReference") === 100) {
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
    assert.deepStrictEqual(vars.map((variable) => variable.name), ["x", "s"]);
  });

  test("findVariable finds a named local and returns undefined otherwise", async () => {
    const local = await findVariable(session, 10, "x");
    assert.strictEqual(local?.value, "1");
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
      return {
        stackFrames: [{ id: 10, name: "main", line: 2, column: 1, source: { path: "/a" } }],
      };
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
      if (numberArg(args, "threadId") === 1) {
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
    await assert.rejects(
      async () => waitForStop(session, 60),
      /waiting for the debuggee to stop/u,
    );
  });
});
