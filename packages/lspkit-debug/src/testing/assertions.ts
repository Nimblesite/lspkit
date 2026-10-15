// High-level DAP assertions built on the dap-client. These turn "inspect a
// local", "evaluate a watch", "we are on line N" into one call with a clear
// failure message — the assertions Osprey/Basilisk/SharpLsp e2e suites repeat.

import * as assert from "assert";
import {
  DapSessionLike,
  DapStackFrame,
  DapVariable,
  evaluate,
  findVariable,
} from "./dap-client";

/** Accepts a substring, a RegExp, or a predicate to match a rendered value. */
export type ValueMatcher = string | RegExp | ((value: string) => boolean);

/** True when `value` satisfies `matcher`. */
export function matchValue(value: string, matcher: ValueMatcher): boolean {
  if (typeof matcher === "string") {
    return value.includes(matcher);
  }
  if (matcher instanceof RegExp) {
    return matcher.test(value);
  }
  return matcher(value);
}

function describeMatcher(matcher: ValueMatcher): string {
  if (typeof matcher === "function") {
    return "predicate";
  }
  return String(matcher);
}

/** Assert a named local/argument exists in `frameId` and matches. Returns it. */
export async function assertLocalVariable(
  session: DapSessionLike,
  frameId: number,
  name: string,
  matcher: ValueMatcher,
): Promise<DapVariable> {
  const variable = await findVariable(session, frameId, name);
  assert.ok(
    variable,
    `expected a local named "${name}" in frame ${frameId}, found none`,
  );
  assert.ok(
    matchValue(variable.value, matcher),
    `local "${name}" = ${JSON.stringify(variable.value)} did not match ${describeMatcher(matcher)}`,
  );
  return variable;
}

/** Assert an evaluated watch expression matches. Returns the result string. */
export async function assertWatch(
  session: DapSessionLike,
  frameId: number,
  expression: string,
  matcher: ValueMatcher,
): Promise<string> {
  const result = await evaluate(session, expression, frameId, "watch");
  assert.ok(
    matchValue(result.result, matcher),
    `watch "${expression}" = ${JSON.stringify(result.result)} did not match ${describeMatcher(matcher)}`,
  );
  return result.result;
}

/** Assert the top frame is on `expectedLine` (and, optionally, in `sourcePath`). */
export function assertCurrentLine(
  stack: { stackFrames: DapStackFrame[] },
  expectedLine: number,
  sourcePath?: string,
): DapStackFrame {
  const top = stack.stackFrames[0];
  assert.ok(top, "expected at least one stack frame");
  assert.strictEqual(top.line, expectedLine, `expected to stop on line ${expectedLine}`);
  if (sourcePath !== undefined) {
    assert.ok(
      top.source?.path && sameFile(top.source.path, sourcePath),
      `expected top frame in ${sourcePath}, was ${top.source?.path}`,
    );
  }
  return top;
}

/** Assert a frame named `name` is somewhere on the stack. Returns it. */
export function assertFunctionOnStack(
  stack: { stackFrames: DapStackFrame[] },
  name: string,
): DapStackFrame {
  const frame = stack.stackFrames.find((f) => f.name.includes(name));
  assert.ok(
    frame,
    `expected a frame named "${name}" on the stack, saw ${stack.stackFrames
      .map((f) => f.name)
      .join(", ")}`,
  );
  return frame;
}

function sameFile(a: string, b: string): boolean {
  const norm = (p: string): string => p.replace(/\\/g, "/").toLowerCase();
  return norm(a) === norm(b);
}
