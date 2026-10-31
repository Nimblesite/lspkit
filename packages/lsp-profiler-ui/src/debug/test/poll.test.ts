import * as assert from "node:assert";
import {
  pollUntilResult,
  sleep,
  waitForCondition,
} from "../poll";

suite("sleep", () => {
  test("resolves after the delay", async () => {
    const start = Date.now();
    await sleep(20);
    assert.ok(Date.now() - start >= 15);
  });
});

suite("pollUntilResult", () => {
  test("object form returns as soon as the predicate accepts", async () => {
    let calls = 0;
    const value = await pollUntilResult({
      fn: async () => {
        calls += 1;
        return calls;
      },
      predicate: (result) => result >= 3,
      intervalMs: 1,
    });
    assert.strictEqual(value, 3);
  });
  test("honours explicit timeout and interval", async () => {
    let calls = 0;
    const value = await pollUntilResult({
      fn: async () => {
        calls += 1;
        return calls;
      },
      predicate: (result) => result >= 2,
      timeoutMs: 1000,
      intervalMs: 1,
    });
    assert.strictEqual(value, 2);
  });
  test("defaults the predicate to always-true (one shot)", async () => {
    const value = await pollUntilResult({ fn: async () => 42 });
    assert.strictEqual(value, 42);
  });
  test("returns the last value when the predicate never holds (no throw)", async () => {
    const value = await pollUntilResult({
      fn: async () => 7,
      predicate: (result) => result === 999,
      timeoutMs: 30,
      intervalMs: 5,
    });
    assert.strictEqual(value, 7);
  });
});

suite("waitForCondition", () => {
  test("returns the value when the predicate eventually holds", async () => {
    let calls = 0;
    const value = await waitForCondition("n reaches 2", {
      fn: async () => {
        calls += 1;
        return calls;
      },
      predicate: (result) => result >= 2,
      timeoutMs: 1000,
      intervalMs: 1,
    });
    assert.strictEqual(value, 2);
  });
  test("throws a labelled error on timeout", async () => {
    await assert.rejects(
      async () =>
        waitForCondition("the impossible", {
          fn: async () => 1,
          predicate: (result) => result === 2,
          timeoutMs: 20,
          intervalMs: 5,
        }),
      /Timed out after 20ms waiting for the impossible; last value: 1/u,
    );
  });
  test("defaults timeout and interval", async () => {
    const value = await waitForCondition("immediate", {
      fn: async () => "ok",
      predicate: (result) => result === "ok",
    });
    assert.strictEqual(value, "ok");
  });
  test("tolerates an unstringifiable last value in its error message", async () => {
    const circular: Record<string, unknown> = {};
    circular.self = circular;
    await assert.rejects(
      async () =>
        waitForCondition("x", {
          fn: async () => circular,
          predicate: () => false,
          timeoutMs: 20,
          intervalMs: 5,
        }),
      /last value: \[object Object\]/u,
    );
  });
});
