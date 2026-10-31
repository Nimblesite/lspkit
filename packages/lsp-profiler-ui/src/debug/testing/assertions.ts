// High-level DAP assertions built on the dap-client. These turn "inspect a
// local", "evaluate a watch", "we are on line N" into one call with a clear
// failure message — the assertions Osprey/Basilisk/SharpLsp e2e suites repeat.

import * as assert from "node:assert";
import { evaluate, findVariable } from "./dap-client";
import type { DapSessionLike, DapStackFrame, DapVariable } from "./dap-client";

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

/** A named local to find, plus the matcher its rendered value must satisfy. */
export interface LocalAssertion {
  name: string;
  matcher: ValueMatcher;
}

/** A watch expression to evaluate, plus the matcher its result must satisfy. */
export interface WatchAssertion {
  expression: string;
  matcher: ValueMatcher;
}

/** Assert a named local/argument exists in `frameId` and matches. Returns it. */
export async function assertLocalVariable(
  session: DapSessionLike,
  frameId: number,
  spec: LocalAssertion,
): Promise<DapVariable> {
  const variable = await findVariable(session, frameId, spec.name);
  assert.ok(
    variable,
    `expected a local named "${spec.name}" in frame ${frameId}, found none`,
  );
  assert.ok(
    matchValue(variable.value, spec.matcher),
    `local "${spec.name}" = ${JSON.stringify(variable.value)} did not match ${describeMatcher(spec.matcher)}`,
  );
  return variable;
}

/** Assert an evaluated watch expression matches. Returns the result string. */
export async function assertWatch(
  session: DapSessionLike,
  frameId: number,
  spec: WatchAssertion,
): Promise<string> {
  const result = await evaluate(session, {
    expression: spec.expression,
    frameId,
    context: "watch",
  });
  assert.ok(
    matchValue(result.result, spec.matcher),
    `watch "${spec.expression}" = ${JSON.stringify(result.result)} did not match ${describeMatcher(spec.matcher)}`,
  );
  return result.result;
}

/** A path in a comparable form: forward slashes, lower case. */
function normalizePath(filePath: string): string {
  return filePath.replaceAll("\\", "/").toLowerCase();
}

function sameFile(left: string, right: string): boolean {
  return normalizePath(left) === normalizePath(right);
}

/** Assert the top frame's source is `sourcePath`, matching either separator. */
function assertSourcePath(frame: DapStackFrame, sourcePath: string): void {
  const actualPath = frame.source?.path;
  assert.ok(
    actualPath !== undefined && sameFile(actualPath, sourcePath),
    `expected top frame in ${sourcePath}, was ${actualPath}`,
  );
}

/** Assert the top frame is on `expectedLine` (and, optionally, in `sourcePath`). */
export function assertCurrentLine(
  stack: { stackFrames: DapStackFrame[] },
  expectedLine: number,
  sourcePath?: string,
): DapStackFrame {
  const [top] = stack.stackFrames;
  assert.ok(stack.stackFrames.length > 0, "expected at least one stack frame");
  assert.strictEqual(top.line, expectedLine, `expected to stop on line ${expectedLine}`);
  if (sourcePath !== undefined) {
    assertSourcePath(top, sourcePath);
  }
  return top;
}

/** The frame names of a stack, for a "saw …" failure message. */
function frameNames(stack: { stackFrames: DapStackFrame[] }): string {
  return stack.stackFrames.map((frame) => frame.name).join(", ");
}

/** Assert a frame named `name` is somewhere on the stack. Returns it. */
export function assertFunctionOnStack(
  stack: { stackFrames: DapStackFrame[] },
  name: string,
): DapStackFrame {
  const frame = stack.stackFrames.find((candidate) => candidate.name.includes(name));
  assert.ok(
    frame,
    `expected a frame named "${name}" on the stack, saw ${frameNames(stack)}`,
  );
  return frame;
}
