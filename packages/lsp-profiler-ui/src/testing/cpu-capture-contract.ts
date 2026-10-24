// Implements [PROF-TESTING]. Identical native-capture assertions in each real VSIX host.
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as vscode from "vscode";
import type { ProfileResult } from "../profiler-decorations";
import { buildFlamegraphHtml, openFlamegraphWebview, flamegraphPanelState } from "../profiler-flamegraph-html";
import { configSection } from "../ids";
import { handleSourceNavigation, SingletonWebviewPanel, buildWebviewDocument } from "../profiler-webview";
import { parseSpeedscope } from "../flame-model";
import { isRecord } from "../unknown-shape";
import { assertHardened, closeAllEditors, pollUntilResult } from "./webview-contract-helpers";

function capturesAttributedSamples(result: ProfileResult): void {
  assert.ok(result.totalSamples > 0);
  assert.ok(result.hotFunctions.some(fn => fn.file !== undefined && fn.file !== "" && fn.percentage > 0));
  const parsed = parseSpeedscope(fs.readFileSync(result.outputFile, "utf8"));
  assert.ok(parsed.ok, parsed.ok ? "" : parsed.error);
  assert.ok(result.cpuProfilePath !== undefined && result.cpuProfilePath !== "");
  const cpu: unknown = JSON.parse(fs.readFileSync(result.cpuProfilePath, "utf8"));
  assert.ok(isRecord(cpu) && Array.isArray(cpu.nodes) && cpu.nodes.length > 0 && Array.isArray(cpu.samples) && cpu.samples.length > 0);
}

function mountsTheInteractiveViews(result: ProfileResult): void {
  openFlamegraphWebview(result);
  const panel = flamegraphPanelState();
  assert.ok(panel !== undefined);
  for (const id of ["flame", "btn-left", "btn-time", "fiber-select", "search", "btn-reset", "trace-actions", "fn-body", "line-body"])
    assert.ok(panel.html.includes(`id="${id}"`), `${id} is reachable in the real host`);
  assertHardened(panel.html, "captured profile");
  const output = process.env.PROFILER_REVIEW_HTML;
  if (output !== undefined && output !== "") fs.writeFileSync(output, buildFlamegraphHtml(result));
}

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
  let observed: string | undefined;
  const panel = new SingletonWebviewPanel(() => "profiler.palette.contract", message => { observed = message.file; });
  panel.show("Palette contract", paletteProbe());
  try {
    await pollUntilResult(() => observed, value => value === "ready");
    await config.update("profiler.palette", { critical: "#74824f" }, vscode.ConfigurationTarget.Global);
    await pollUntilResult(() => observed, value => value === "#74824f");
    assert.equal(observed, "#74824f");
  } finally { panel.dispose(); await config.update("profiler.palette", previous, vscode.ConfigurationTarget.Global); }
}

async function hotLinesNavigateToTheirSource(result: ProfileResult): Promise<void> {
  const line = result.hotLines.find(item => item.line > 0 && fs.existsSync(item.file));
  assert.ok(line !== undefined, "capture must attribute a real source line");
  assert.equal(handleSourceNavigation({ type: "navigateToSource", file: line.file, line: line.line }), true);
  const editor = await pollUntilResult(() => vscode.window.activeTextEditor,
    active => active?.document.uri.fsPath === line.file && active.selection.active.line === line.line - 1);
  assert.equal(editor?.selection.active.line, line.line - 1);
}

/** Register the contract against `capture`, the host's own real runtime capture. */
export function registerCpuCaptureContract(capture: () => Promise<ProfileResult>): void {
  suite("Shared profiler VSIX — real runtime capture", () => {
    let result: ProfileResult;
    suiteSetup(async function () { this.timeout(120_000); result = await capture(); });
    suiteTeardown(closeAllEditors);
    test("captures attributed CPU samples and valid native trace artifacts", () => { capturesAttributedSamples(result); });
    test("mounts both interactive views, search, fiber selection and trace actions", () => { mountsTheInteractiveViews(result); });
    test("palette overrides update an already-open webview", paletteOverridesRepaintOpenPanels);
    test("a measured hot line navigates to the actual runtime source", async () => { await hotLinesNavigateToTheirSource(result); });
  });
}
