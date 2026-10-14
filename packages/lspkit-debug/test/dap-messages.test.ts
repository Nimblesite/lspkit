import * as assert from "assert";
import { summarizeArgs, summarizeBody } from "../src/dap-messages";

suite("summarizeArgs", () => {
  test("renders known scalar fields and breakpoint counts", () => {
    const s = summarizeArgs({ threadId: 1, frameId: 2, expression: "x", breakpoints: [{}, {}] });
    assert.ok(s.includes("threadId=1"));
    assert.ok(s.includes("frameId=2"));
    assert.ok(s.includes("expression=x"));
    assert.ok(s.includes("breakpoints=2"));
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
    const s = summarizeBody({
      reason: "breakpoint",
      threadId: 1,
      stackFrames: [{}, {}, {}],
      variables: [{}],
    });
    assert.ok(s.includes("reason=breakpoint"));
    assert.ok(s.includes("threadId=1"));
    assert.ok(s.includes("stackFrames=3"));
    assert.ok(s.includes("variables=1"));
  });
  test("renders an evaluate result string", () => {
    assert.ok(summarizeBody({ result: "42" }).includes("result=42"));
  });
  test("returns empty string for non-objects", () => {
    assert.strictEqual(summarizeBody("nope"), "");
  });
});
