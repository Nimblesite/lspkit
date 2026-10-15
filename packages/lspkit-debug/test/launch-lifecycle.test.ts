import * as assert from "assert";
import {
  runPreLaunchBuild,
  saveDirtyOrReject,
} from "../src/launch-lifecycle";

suite("saveDirtyOrReject", () => {
  test("treats a missing document as clean", async () => {
    const rejects: string[] = [];
    assert.strictEqual(await saveDirtyOrReject(undefined, (m) => rejects.push(m)), true);
    assert.strictEqual(rejects.length, 0);
  });
  test("treats an already-clean document as clean without saving", async () => {
    let saved = false;
    const ok = await saveDirtyOrReject(
      { isDirty: false, save: () => { saved = true; return Promise.resolve(true); } },
      () => undefined,
    );
    assert.strictEqual(ok, true);
    assert.strictEqual(saved, false);
  });
  test("saves a dirty document and proceeds on success", async () => {
    const ok = await saveDirtyOrReject(
      { isDirty: true, save: () => Promise.resolve(true) },
      () => undefined,
    );
    assert.strictEqual(ok, true);
  });
  test("rejects with a message when the save fails", async () => {
    const rejects: string[] = [];
    const ok = await saveDirtyOrReject(
      { isDirty: true, save: () => Promise.resolve(false) },
      (m) => rejects.push(m),
    );
    assert.strictEqual(ok, false);
    assert.deepStrictEqual(rejects, ["Save the source file before debugging."]);
  });
  test("honours a custom reject message", async () => {
    const rejects: string[] = [];
    await saveDirtyOrReject(
      { isDirty: true, save: () => Promise.resolve(false) },
      (m) => rejects.push(m),
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
      async (a) => { seen.push(a); },
      args,
      () => undefined,
    );
    assert.strictEqual(ok, true);
    assert.deepStrictEqual(seen, [args]);
  });
  test("maps an Error to onError and resolves false", async () => {
    const errors: string[] = [];
    const ok = await runPreLaunchBuild(
      async () => { throw new Error("compile failed"); },
      args,
      (m) => errors.push(m),
    );
    assert.strictEqual(ok, false);
    assert.deepStrictEqual(errors, ["compile failed"]);
  });
  test("stringifies a non-Error rejection", async () => {
    const errors: string[] = [];
    await runPreLaunchBuild(
      () => Promise.reject("boom"),
      args,
      (m) => errors.push(m),
    );
    assert.deepStrictEqual(errors, ["boom"]);
  });
});
