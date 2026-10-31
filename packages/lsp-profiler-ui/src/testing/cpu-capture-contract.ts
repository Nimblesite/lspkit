// Implements [PROF-TESTING]. Identical native-capture assertions in each real VSIX host.
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as vscode from "vscode";
import type { ProfileResult } from "../profiler-decorations";
import { configSection } from "../ids";
import { editorPathKey } from "../editor-path-key";
import { SingletonWebviewPanel, buildWebviewDocument, handleSourceNavigation } from "../profiler-webview";
import { closeAllEditors, pollUntilResult } from "./webview-contract-helpers";
import { capturesAttributedSamples, mountsTheInteractiveViews } from "./cpu-capture-assertions";

/** A panel whose script reports `--prof-critical` back each time the palette is repainted. */
function paletteProbe(): string {
  return buildWebviewDocument({ title: "Palette", css: "", body: "", script: `
  const api = globalThis.profilerVsCodeApi || (globalThis.profilerVsCodeApi = acquireVsCodeApi());
  window.addEventListener('message', event => { if(event.data.type === 'profilerPalette')
    api.postMessage({type:'palette', file:document.documentElement.style.getPropertyValue('--prof-critical')}); });
  api.postMessage({type:'ready', file:'ready'});` });
}

async function paletteOverridesRepaintOpenPanels(): Promise<void> {
  const config = vscode.workspace.getConfiguration(configSection());
  const previous = config.inspect("profiler.palette")?.globalValue;
  let observed: string | undefined = undefined;
  const panel = new SingletonWebviewPanel(() => "profiler.palette.contract", message => { observed = message.file; });
  panel.show("Palette contract", paletteProbe());
  try {
    await repaintPaletteProbe(config, () => observed);
  } finally {
    panel.dispose();
    await config.update("profiler.palette", previous, vscode.ConfigurationTarget.Global);
  }
}

/** Change the palette and wait for the open panel to repaint with it. */
async function repaintPaletteProbe(config: vscode.WorkspaceConfiguration, observed: () => string | undefined): Promise<void> {
  await pollUntilResult(observed, value => value === "ready");
  await config.update("profiler.palette", { critical: "#74824f" }, vscode.ConfigurationTarget.Global);
  await pollUntilResult(observed, value => value === "#74824f");
  assert.equal(observed(), "#74824f");
}

async function fileExists(filePath: string): Promise<boolean> {
  try {
    await fs.promises.access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function hotLinesNavigateToTheirSource(result: ProfileResult): Promise<void> {
  const located = await Promise.all(
    result.hotLines
      .filter(item => item.line > 0)
      .map(async item => ({ item, exists: await fileExists(item.file) })),
  );
  const line = located.find(entry => entry.exists)?.item;
  assert.ok(line !== undefined, "capture must attribute a real source line");
  assert.equal(handleSourceNavigation({ type: "navigateToSource", file: line.file, line: line.line }), true);
  // The editor spells a Windows drive in lower case; the runtime reports it as built.
  const opened = (active: vscode.TextEditor | undefined): boolean =>
    active !== undefined && editorPathKey(active.document.uri.fsPath) === editorPathKey(line.file);
  const editor = await pollUntilResult(() => vscode.window.activeTextEditor,
    active => opened(active) && active?.selection.active.line === line.line - 1);
  assert.ok(opened(editor), `the hot line's own file opens: ${line.file}`);
  assert.equal(editor?.selection.active.line, line.line - 1);
}

/** How long the one real CPU capture may take on a cold CI runner. */
const CPU_CAPTURE_TIMEOUT_MS = 120_000;

/** Register the contract against `capture`, the host's own real runtime capture. */
export function registerCpuCaptureContract(capture: () => Promise<ProfileResult>): void {
  suite("Shared profiler VSIX — real runtime capture", () => {
    const captured: { result: ProfileResult | undefined } = { result: undefined };
    suiteSetup(async function captureProfile() { this.timeout(CPU_CAPTURE_TIMEOUT_MS); captured.result = await capture(); });
    suiteTeardown(closeAllEditors);
    test("captures attributed CPU samples and valid native trace artifacts", async () => { await capturesAttributedSamples(contractResult(captured)); });
    test("mounts both interactive views, search, fiber selection and trace actions", async () => { await mountsTheInteractiveViews(contractResult(captured)); });
    test("palette overrides update an already-open webview", paletteOverridesRepaintOpenPanels);
    test("a measured hot line navigates to the actual runtime source", async () => { await hotLinesNavigateToTheirSource(contractResult(captured)); });
  });
}

/** The captured result, failing loudly when the suite setup never produced one. */
function contractResult(captured: { result: ProfileResult | undefined }): ProfileResult {
  if (captured.result === undefined) { throw new Error("the capture suite setup did not produce a result"); }
  return captured.result;
}
