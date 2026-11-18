// Tests [PROF-UI-LIFETIME]: heat decorations are keyed by colour, and the
// palette is re-read on every render, so a palette change must not leave the
// previous colours' decoration types allocated for the rest of the session.
import * as assert from "node:assert";
import * as vscode from "vscode";
import type { ProfileResult } from "../../profiler-decorations";
import { applyProfileDecorations, clearProfileDecorations, profileDecorationTypeCount } from "../../profiler-decorations";
import { closeAllEditors, openSourceFile, setupLspTestSuite, teardownLspTestSuite } from "./test-helpers";

/** Palette changes a long session sees. */
const PALETTE_CHANGES = 8;
/** Heat levels the decorations paint: critical, hot, warm and cool. */
const HEAT_LEVELS = 4;
/** The user setting the palette is read from. */
const PALETTE_SECTION = "lspui.profiler";

/** A six-digit hex colour unique to `index`. */
function colour(index: number): string {
  return `#0000${index.toString(16).padStart(2, "0")}`;
}

/** Change the palette and paint `result`, once per change, each landing before the next. */
async function paintUnderPalettes(result: ProfileResult, index: number): Promise<void> {
  if (index >= PALETTE_CHANGES) { return; }
  await vscode.workspace
    .getConfiguration(PALETTE_SECTION)
    .update("palette", { critical: colour(index) }, vscode.ConfigurationTarget.Global);
  await applyProfileDecorations(result);
  await paintUnderPalettes(result, index + 1);
}

suite("Profiler — decoration types follow the palette", () => {
  let tmpDir = "";

  suiteSetup(async () => {
    ({ tmpDir } = await setupLspTestSuite("lspui-palette-"));
  });

  suiteTeardown(async () => {
    await vscode.workspace.getConfiguration(PALETTE_SECTION).update("palette", undefined, vscode.ConfigurationTarget.Global);
    clearProfileDecorations();
    await closeAllEditors();
    await teardownLspTestSuite(tmpDir);
  });

  test("palette changes never accumulate decoration types", async () => {
    const { uri } = await openSourceFile(tmpDir, "palette_hot.py", "def hot():\n    return 1\n");
    const result: ProfileResult = {
      sessionId: "palette-session",
      duration: 1,
      totalSamples: 100,
      outputFile: "",
      hotFunctions: [],
      hotLines: [{ file: uri.fsPath, line: 1, samples: 90, percentage: 90 }],
    };
    await paintUnderPalettes(result, 0);
    assert.ok(
      profileDecorationTypeCount() <= HEAT_LEVELS,
      `${String(profileDecorationTypeCount())} decoration types allocated after ${String(PALETTE_CHANGES)} palette changes`,
    );
  });
});
