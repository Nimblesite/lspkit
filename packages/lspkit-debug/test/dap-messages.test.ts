import * as assert from "node:assert";
import { summarizeArgs, summarizeBody } from "../src/dap-messages";

suite("summarizeArgs", () => {
  test("renders known scalar fields and breakpoint counts", () => {
    const summary = summarizeArgs({ threadId: 1, frameId: 2, expression: "x", breakpoints: [{}, {}] });
    assert.ok(summary.includes("threadId=1"));
    assert.ok(summary.includes("frameId=2"));
    assert.ok(summary.includes("expression=x"));
    assert.ok(summary.includes("breakpoints=2"));
  });
  test("returns empty string for non-objects", () => {
    assert.strictEqual(summarizeArgs(undefined), "");
    assert.strictEqual(summarizeArgs(null), "");
    assert.strictEqual(summarizeArgs(7), "");
  });
  test("omits absent fields", () => {
    assert.strictEqual(summarizeArgs({ variablesReference: 9 }), "variablesReference=9");
  });
});

suite("summarizeBody", () => {
  test("renders reason, threadId and array counts", () => {
    const summary = summarizeBody({
      reason: "breakpoint",
      threadId: 1,
      stackFrames: [{}, {}, {}],
      variables: [{}],
    });
    assert.ok(summary.includes("reason=breakpoint"));
    assert.ok(summary.includes("threadId=1"));
    assert.ok(summary.includes("stackFrames=3"));
    assert.ok(summary.includes("variables=1"));
  });
  test("renders an evaluate result string", () => {
    assert.ok(summarizeBody({ result: "42" }).includes("result=42"));
  });
  test("returns empty string for non-objects", () => {
    assert.strictEqual(summarizeBody("nope"), "");
  });
});
