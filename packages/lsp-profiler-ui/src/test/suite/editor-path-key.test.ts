// Tests [PROF-UI-HEATMAP]: runtime and editor paths identify the same source.
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { editorPathKey, pathKeyer } from "../../editor-path-key";

const FIXTURE_FILES = ["Program.cs", "HotLeakApp.csproj"];

/**
 * Remove `temporary` and the link inside it, never what the link points at. On
 * Windows a recursive delete follows a junction into its target: it deleted the
 * real fixture project on every run.
 */
function removeLinked(temporary: string, linked: string): void {
  if (fs.lstatSync(linked, { throwIfNoEntry: false })?.isSymbolicLink() === true) { fs.unlinkSync(linked); }
  fs.rmSync(temporary, { recursive: true, force: true });
}

suite("Editor source paths across symlinks and junctions", () => {
  const source = path.resolve(__dirname, "../../../../../../dotnet/fixtures/HotLeakApp");
  let temporary: string;
  let linked: string;
  setup(() => {
    temporary = fs.mkdtempSync(path.join(os.tmpdir(), "profiler-path-"));
    linked = path.join(temporary, "linked-project");
  });
  teardown(() => { removeLinked(temporary, linked); });

  test("a linked project and its canonical source share the heat-map key", () => {
    fs.symlinkSync(source, linked, process.platform === "win32" ? "junction" : "dir");
    const key = pathKeyer();
    for (const name of FIXTURE_FILES) {
      const original = path.join(source, name);
      const alias = path.join(linked, name);
      assert.ok(fs.existsSync(alias), "the test opens a real project through its link");
      assert.equal(key(alias), key(original), "both producers must match the same file");
      assert.equal(key(alias), editorPathKey(fs.realpathSync.native(original)));
    }
  });

  test("a deleted source falls back to a stable resolved key", () => {
    const absent = path.join(temporary, "deleted.cs");
    assert.equal(editorPathKey(absent), editorPathKey(path.join(temporary, ".", "deleted.cs")));
    assert.notEqual(editorPathKey(absent), editorPathKey(path.join(temporary, "other.cs")));
  });

  test("removing the link leaves the project it linked to in place", () => {
    fs.symlinkSync(source, linked, process.platform === "win32" ? "junction" : "dir");
    removeLinked(temporary, linked);
    assert.equal(fs.existsSync(temporary), false, "the temporary directory is gone");
    for (const name of FIXTURE_FILES) {
      assert.ok(fs.existsSync(path.join(source, name)), `the linked project keeps ${name}`);
    }
  });
});
