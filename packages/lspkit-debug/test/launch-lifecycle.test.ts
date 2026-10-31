import * as assert from "node:assert";
import {
  runPreLaunchBuild,
  saveDirtyOrReject,
} from "../src/launch-lifecycle";

/** A rejection handler for cases where the launch must not be rejected. */
function recordNothing(): void {
  // Nothing to record: this launch is expected to proceed.
}

suite("saveDirtyOrReject", () => {
  test("treats a missing document as clean", async () => {
    const rejects: string[] = [];
    assert.strictEqual(
      await saveDirtyOrReject(undefined, (message) => {
        rejects.push(message);
      }),
      true,
    );
    assert.strictEqual(rejects.length, 0);
  });
  test("treats an already-clean document as clean without saving", async () => {
    let saved = false;
    const ok = await saveDirtyOrReject(
      { isDirty: false, save: async () => { saved = true; return true; } },
      recordNothing,
    );
    assert.strictEqual(ok, true);
    assert.strictEqual(saved, false);
  });
  test("saves a dirty document and proceeds on success", async () => {
    const ok = await saveDirtyOrReject(
      { isDirty: true, save: async () => true },
      recordNothing,
    );
    assert.strictEqual(ok, true);
  });
  test("rejects with a message when the save fails", async () => {
    const rejects: string[] = [];
    const ok = await saveDirtyOrReject(
      { isDirty: true, save: async () => false },
      (message) => {
        rejects.push(message);
      },
    );
    assert.strictEqual(ok, false);
    assert.deepStrictEqual(rejects, ["Save the source file before debugging."]);
  });
  test("honours a custom reject message", async () => {
    const rejects: string[] = [];
    await saveDirtyOrReject(
      { isDirty: true, save: async () => false },
      (message) => {
        rejects.push(message);
      },
      "Save first!",
    );
    assert.deepStrictEqual(rejects, ["Save first!"]);
  });
});

suite("runPreLaunchBuild", () => {
  const args = { sourceProgram: "/a.osp", debugOutput: "/out/a", cwd: "/" };
  test("resolves true after a clean build", async () => {
    const seen: typeof args[] = [];
    const ok = await runPreLaunchBuild(
      async (buildArgs) => { seen.push(buildArgs); },
      args,
      recordNothing,
    );
    assert.strictEqual(ok, true);
    assert.deepStrictEqual(seen, [args]);
  });
  test("maps an Error to onError and resolves false", async () => {
    const errors: string[] = [];
    const ok = await runPreLaunchBuild(
      async () => { throw new Error("compile failed"); },
      args,
      (message) => {
        errors.push(message);
      },
    );
    assert.strictEqual(ok, false);
    assert.deepStrictEqual(errors, ["compile failed"]);
  });
  test("stringifies a non-Error rejection", async () => {
    const errors: string[] = [];
    const reason: unknown = "boom";
    await runPreLaunchBuild(
      async () => {
        throw reason;
      },
      args,
      (message) => {
        errors.push(message);
      },
    );
    assert.deepStrictEqual(errors, ["boom"]);
  });
});
