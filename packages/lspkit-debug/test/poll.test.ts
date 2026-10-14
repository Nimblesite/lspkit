import * as assert from "assert";
import {
  pollUntilResult,
  sleep,
  waitForCondition,
} from "../src/poll";

suite("sleep", () => {
  test("resolves after the delay", async () => {
    const start = Date.now();
    await sleep(20);
    assert.ok(Date.now() - start >= 15);
  });
});

suite("pollUntilResult", () => {
  test("object form returns as soon as the predicate accepts", async () => {
    let n = 0;
    const value = await pollUntilResult({
      fn: () => Promise.resolve(++n),
      predicate: (v) => v >= 3,
      intervalMs: 1,
    });
    assert.strictEqual(value, 3);
  });
  test("positional form works with predicate + timings", async () => {
    let n = 0;
    const value = await pollUntilResult(
      () => Promise.resolve(++n),
      (v) => v >= 2,
      1000,
      1,
    );
    assert.strictEqual(value, 2);
  });
  test("positional form defaults the predicate to always-true (one shot)", async () => {
    const value = await pollUntilResult(() => Promise.resolve(42));
    assert.strictEqual(value, 42);
  });
  test("returns the last value when the predicate never holds (no throw)", async () => {
    const value = await pollUntilResult(
      () => Promise.resolve(7),
      (v) => v === 999,
      30,
      5,
    );
    assert.strictEqual(value, 7);
  });
});

suite("waitForCondition", () => {
  test("returns the value when the predicate eventually holds", async () => {
    let n = 0;
    const value = await waitForCondition(
      "n reaches 2",
      () => Promise.resolve(++n),
      (v) => v >= 2,
      1000,
      1,
    );
    assert.strictEqual(value, 2);
  });
  test("throws a labelled error on timeout", async () => {
    await assert.rejects(
      () =>
        waitForCondition(
          "the impossible",
          () => Promise.resolve(1),
          (v) => v === 2,
          20,
          5,
        ),
      /Timed out after 20ms waiting for the impossible; last value: 1/,
    );
  });
  test("object form is accepted", async () => {
    const value = await waitForCondition("immediate", {
      fn: () => Promise.resolve("ok"),
      predicate: (v) => v === "ok",
    });
    assert.strictEqual(value, "ok");
  });
  test("tolerates an unstringifiable last value in its error message", async () => {
    const circular: Record<string, unknown> = {};
    circular.self = circular;
    await assert.rejects(
      () => waitForCondition("x", () => Promise.resolve(circular), () => false, 20, 5),
      /last value: \[object Object\]/,
    );
  });
});
