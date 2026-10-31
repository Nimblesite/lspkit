import * as assert from "node:assert";
import {
  assertCurrentLine,
  assertFunctionOnStack,
  assertLocalVariable,
  assertWatch,
  matchValue,
} from "../src/testing/assertions";
import type { DapSessionLike } from "../src/testing/dap-client";

/** True when `value` is a non-null object whose fields can be inspected. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** The expression an evaluate request was issued for. */
function expressionArg(args: unknown): unknown {
  return isRecord(args) ? args.expression : undefined;
}

class FakeSession implements DapSessionLike {
  private readonly handler: (command: string, args: unknown) => unknown;

  public constructor(handler: (command: string, args: unknown) => unknown) {
    this.handler = handler;
  }

  public async customRequest(command: string, args?: unknown): Promise<unknown> {
    return this.handler(command, args);
  }
}

const session = new FakeSession((command, args) => {
  if (command === "scopes") {
    return { scopes: [{ name: "Locals", variablesReference: 100 }] };
  }
  if (command === "variables") {
    return {
      variables: [
        { name: "x", value: "1", variablesReference: 0 },
        { name: "s", value: '"hi"', variablesReference: 0 },
      ],
    };
  }
  if (command === "evaluate") {
    return {
      result: expressionArg(args) === "x * 2" ? "2" : "<err>",
      variablesReference: 0,
    };
  }
  return {};
});

suite("matchValue", () => {
  test("substring, regexp, and predicate forms", () => {
    assert.ok(matchValue("hello world", "world"));
    assert.ok(matchValue("value=42", /\d+/u));
    assert.ok(matchValue("abc", (value) => value.length === 3));
    assert.ok(!matchValue("abc", "z"));
  });
});

suite("assertLocalVariable", () => {
  test("returns the matching local", async () => {
    const variable = await assertLocalVariable(session, 10, { name: "x", matcher: /1/u });
    assert.strictEqual(variable.value, "1");
    await assertLocalVariable(session, 10, { name: "s", matcher: "hi" });
    await assertLocalVariable(session, 10, { name: "x", matcher: (value) => value === "1" });
  });
  test("throws when the local is absent", async () => {
    await assert.rejects(
      async () => assertLocalVariable(session, 10, { name: "ghost", matcher: /x/u }),
      /expected a local named "ghost"/u,
    );
  });
  test("throws when the value does not match", async () => {
    await assert.rejects(
      async () => assertLocalVariable(session, 10, { name: "x", matcher: "999" }),
      /did not match 999/u,
    );
  });
});

suite("assertWatch", () => {
  test("returns the evaluated result when it matches", async () => {
    assert.strictEqual(await assertWatch(session, 10, { expression: "x * 2", matcher: /2/u }), "2");
  });
  test("throws on a mismatch and names the expression", async () => {
    await assert.rejects(
      async () => assertWatch(session, 10, { expression: "nope", matcher: /2/u }),
      /watch "nope"/u,
    );
  });
});

suite("assertCurrentLine / assertFunctionOnStack", () => {
  const stack = {
    stackFrames: [
      { id: 1, name: "inner", line: 7, column: 3, source: { path: "/src/a.osp" } },
      { id: 2, name: "main", line: 12, column: 1, source: { path: "/src/a.osp" } },
    ],
  };
  test("asserts the top line and optional source", () => {
    const top = assertCurrentLine(stack, 7, "/src/a.osp");
    assert.strictEqual(top.id, 1);
    // The source path is optional.
    assertCurrentLine(stack, 7);
  });
  test("throws on the wrong line", () => {
    assert.throws(() => assertCurrentLine(stack, 99), /stop on line 99/u);
  });
  test("throws when the source path differs", () => {
    assert.throws(() => assertCurrentLine(stack, 7, "/other.osp"), /expected top frame/u);
  });
  test("throws on an empty stack", () => {
    assert.throws(() => assertCurrentLine({ stackFrames: [] }, 1), /at least one stack frame/u);
  });
  test("finds a function frame anywhere on the stack", () => {
    assert.strictEqual(assertFunctionOnStack(stack, "main").id, 2);
  });
  test("throws when no frame matches", () => {
    assert.throws(
      () => assertFunctionOnStack(stack, "absent"),
      /expected a frame named "absent"/u,
    );
  });
});
