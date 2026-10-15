// Tests [PROF-UI-HEATMAP]: runtime and editor paths identify the same source.
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { editorPathKey, pathKeyer } from "../../editor-path-key";

suite("Editor source paths across symlinks and junctions", () => {
  const source = path.resolve(__dirname, "../../../../../../dotnet/fixtures/HotLeakApp");
  let temporary: string;
  setup(() => { temporary = fs.mkdtempSync(path.join(os.tmpdir(), "profiler-path-")); });
  teardown(() => { fs.rmSync(temporary, { recursive: true, force: true }); });

  test("a linked project and its canonical source share the heat-map key", () => {
    const linked = path.join(temporary, "linked-project");
    fs.symlinkSync(source, linked, process.platform === "win32" ? "junction" : "dir");
    const key = pathKeyer();
    for (const name of ["Program.cs", "HotLeakApp.csproj"]) {
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
});
