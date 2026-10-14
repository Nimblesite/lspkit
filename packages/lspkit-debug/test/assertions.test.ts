import * as assert from "assert";
import {
  assertCurrentLine,
  assertFunctionOnStack,
  assertLocalVariable,
  assertWatch,
  matchValue,
} from "../src/testing/assertions";
import { DapSessionLike } from "../src/testing/dap-client";

class FakeSession implements DapSessionLike {
  constructor(private readonly handler: (command: string, args: unknown) => unknown) {}
  customRequest(command: string, args?: unknown): Promise<unknown> {
    return Promise.resolve(this.handler(command, args));
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
    const expr = (args as { expression: string }).expression;
    return { result: expr === "x * 2" ? "2" : "<err>", variablesReference: 0 };
  }
  return {};
});

suite("matchValue", () => {
  test("substring, regexp, and predicate forms", () => {
    assert.ok(matchValue("hello world", "world"));
    assert.ok(matchValue("value=42", /\d+/));
    assert.ok(matchValue("abc", (v) => v.length === 3));
    assert.ok(!matchValue("abc", "z"));
  });
});

suite("assertLocalVariable", () => {
  test("returns the matching local", async () => {
    const v = await assertLocalVariable(session, 10, "x", /1/);
    assert.strictEqual(v.value, "1");
    await assertLocalVariable(session, 10, "s", "hi");
    await assertLocalVariable(session, 10, "x", (val) => val === "1");
  });
  test("throws when the local is absent", async () => {
    await assert.rejects(
      () => assertLocalVariable(session, 10, "ghost", /x/),
      /expected a local named "ghost"/,
    );
  });
  test("throws when the value does not match", async () => {
    await assert.rejects(
      () => assertLocalVariable(session, 10, "x", "999"),
      /did not match 999/,
    );
  });
});

suite("assertWatch", () => {
  test("returns the evaluated result when it matches", async () => {
    assert.strictEqual(await assertWatch(session, 10, "x * 2", /2/), "2");
  });
  test("throws on a mismatch and names the expression", async () => {
    await assert.rejects(
      () => assertWatch(session, 10, "nope", /2/),
      /watch "nope"/,
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
    assertCurrentLine(stack, 7); // source optional
  });
  test("throws on the wrong line", () => {
    assert.throws(() => assertCurrentLine(stack, 99), /stop on line 99/);
  });
  test("throws when the source path differs", () => {
    assert.throws(() => assertCurrentLine(stack, 7, "/other.osp"), /expected top frame/);
  });
  test("throws on an empty stack", () => {
    assert.throws(() => assertCurrentLine({ stackFrames: [] }, 1), /at least one stack frame/);
  });
  test("finds a function frame anywhere on the stack", () => {
    assert.strictEqual(assertFunctionOnStack(stack, "main").id, 2);
  });
  test("throws when no frame matches", () => {
    assert.throws(() => assertFunctionOnStack(stack, "absent"), /expected a frame named "absent"/);
  });
});
