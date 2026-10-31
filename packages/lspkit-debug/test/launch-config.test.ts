import * as assert from "node:assert";
import path from "node:path";
import {
  applyLaunchConfigDefaults,
  defaultDebugOutputPath,
  defaultLaunchConfigForEditor,
  isBlankConfig,
} from "../src/launch-config";
import type {
  ActiveEditorLike,
  DebugDocumentLike,
  LaunchConfigPolicy,
} from "../src/launch-config";

const policy: LaunchConfigPolicy = {
  type: "osprey",
  name: "Debug Osprey File",
  isLanguageDocument: (doc: DebugDocumentLike) =>
    doc.languageId === "osprey" || doc.fileName.endsWith(".osp"),
};

const editor = (fileName: string, languageId = "osprey"): ActiveEditorLike => ({
  document: { languageId, fileName },
});

suite("isBlankConfig", () => {
  test("true when type/request/name are all absent", () => {
    assert.ok(isBlankConfig({}));
  });
  test("false when any of type/request/name is present", () => {
    assert.ok(!isBlankConfig({ type: "osprey" }));
    assert.ok(!isBlankConfig({ request: "launch" }));
    assert.ok(!isBlankConfig({ name: "x" }));
  });
});

suite("applyLaunchConfigDefaults", () => {
  test("synthesizes a full config from an osprey editor", () => {
    const config = applyLaunchConfigDefaults({}, editor("/tmp/main.osp"), policy);
    assert.strictEqual(config.type, "osprey");
    assert.strictEqual(config.request, "launch");
    assert.strictEqual(config.name, "Debug Osprey File");
    assert.strictEqual(config.program, "/tmp/main.osp");
    assert.strictEqual(config.cwd, path.dirname("/tmp/main.osp"));
  });
  test("honours a custom request and deriveFields", () => {
    const config = applyLaunchConfigDefaults({}, editor("/tmp/a.osp"), {
      ...policy,
      request: "launch-noopt",
      deriveFields: (program) => ({ cwd: "/work", sourceProgram: program }),
    });
    assert.strictEqual(config.request, "launch-noopt");
    assert.strictEqual(config.cwd, "/work");
    assert.strictEqual(config.sourceProgram, "/tmp/a.osp");
  });
  test("leaves a non-language editor untouched", () => {
    const config = applyLaunchConfigDefaults({}, editor("/tmp/notes.txt", "plaintext"), policy);
    assert.deepStrictEqual(config, {});
  });
  test("recognizes a .osp file even with a foreign languageId", () => {
    const config = applyLaunchConfigDefaults({}, editor("/tmp/x.osp", "plaintext"), policy);
    assert.strictEqual(config.type, "osprey");
  });
  test("leaves an empty config untouched when there is no active editor", () => {
    assert.deepStrictEqual(applyLaunchConfigDefaults({}, undefined, policy), {});
  });
  test("never overwrites an already-populated config", () => {
    const preset = { type: "osprey", request: "launch", name: "Custom", program: "/explicit.osp" };
    const config = applyLaunchConfigDefaults({ ...preset }, editor("/tmp/other.osp"), policy);
    assert.strictEqual(config.program, "/explicit.osp");
    assert.strictEqual(config.name, "Custom");
  });
});

suite("defaultLaunchConfigForEditor", () => {
  test("builds a config from the editor", () => {
    const config = defaultLaunchConfigForEditor(editor("/tmp/m.osp"), policy);
    assert.strictEqual(config.program, "/tmp/m.osp");
  });
  test("returns {} for no editor", () => {
    assert.deepStrictEqual(defaultLaunchConfigForEditor(undefined, policy), {});
  });
});

suite("defaultDebugOutputPath", () => {
  test("places the artifact under the default subdir without extension on posix", () => {
    const out = defaultDebugOutputPath(path.join("/tmp", "demo.osp"), "linux");
    assert.strictEqual(path.basename(path.dirname(out)), ".lspkit-debug");
    assert.strictEqual(path.basename(out), "demo");
  });
  test("adds .exe on win32 and honours a custom subdir", () => {
    const out = defaultDebugOutputPath(path.join("/tmp", "demo.osp"), "win32", ".osprey-debug");
    assert.strictEqual(path.basename(out), "demo.exe");
    assert.strictEqual(path.basename(path.dirname(out)), ".osprey-debug");
  });
});
