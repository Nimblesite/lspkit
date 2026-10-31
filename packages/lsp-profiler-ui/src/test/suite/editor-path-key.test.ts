// Tests [PROF-UI-HEATMAP]: runtime and editor paths identify the same source.
import * as assert from "node:assert/strict";
import * as fs from "node:fs/promises";
import type { Stats } from "node:fs";
import os from "node:os";
import path from "node:path";
import { editorPathKey, pathKeyer } from "../../editor-path-key";

const FIXTURE_FILES = ["Program.cs", "HotLeakApp.csproj"];
const LINK_TYPE = process.platform === "win32" ? "junction" : "dir";
const LINKED_DIRECTORY = "linked-project";
const TEMPORARY_PREFIX = "profiler-path-";

/** Stats for `target`, or `undefined` when nothing is there. */
async function statsOf(target: string): Promise<Stats | undefined> {
  try {
    return await fs.lstat(target);
  } catch {
    return undefined;
  }
}

async function exists(target: string): Promise<boolean> {
  return (await statsOf(target)) !== undefined;
}

async function linkProject(source: string, linked: string): Promise<void> {
  await fs.symlink(source, linked, LINK_TYPE);
}

/**
 * Remove `temporary` and the link inside it, never what the link points at. On
 * Windows a recursive delete follows a junction into its target: it deleted the
 * real fixture project on every run.
 */
async function removeLinked(temporary: string, linked: string): Promise<void> {
  const linkedStats = await statsOf(linked);
  if (linkedStats?.isSymbolicLink() === true) {
    await fs.unlink(linked);
  }
  await fs.rm(temporary, { recursive: true, force: true });
}

suite("Editor source paths across symlinks and junctions", () => {
  const source = path.resolve(__dirname, "../../../../../../dotnet/fixtures/HotLeakApp");
  let temporary = "";
  let linked = "";
  setup(async () => {
    temporary = await fs.mkdtemp(path.join(os.tmpdir(), TEMPORARY_PREFIX));
    linked = path.join(temporary, LINKED_DIRECTORY);
  });
  teardown(async () => {
    await removeLinked(temporary, linked);
  });

  test("a linked project and its canonical source share the heat-map key", async () => {
    await linkProject(source, linked);
    const files = FIXTURE_FILES.flatMap((name) => [path.join(source, name), path.join(linked, name)]);
    const key = await pathKeyer(files);
    await Promise.all(
      FIXTURE_FILES.map(async (name) => {
        const original = path.join(source, name);
        const alias = path.join(linked, name);
        assert.ok(await exists(alias), "the test opens a real project through its link");
        assert.equal(key(alias), key(original), "both producers must match the same file");
        assert.equal(key(alias), await editorPathKey(await fs.realpath(original)));
      }),
    );
  });

  test("a deleted source falls back to a stable resolved key", async () => {
    const absent = path.join(temporary, "deleted.cs");
    assert.equal(await editorPathKey(absent), await editorPathKey(path.join(temporary, ".", "deleted.cs")));
    assert.notEqual(await editorPathKey(absent), await editorPathKey(path.join(temporary, "other.cs")));
  });

  test("removing the link leaves the project it linked to in place", async () => {
    await linkProject(source, linked);
    await removeLinked(temporary, linked);
    assert.equal(await exists(temporary), false, "the temporary directory is gone");
    await Promise.all(
      FIXTURE_FILES.map(async (name) => {
        assert.ok(await exists(path.join(source, name)), `the linked project keeps ${name}`);
      }),
    );
  });
});
