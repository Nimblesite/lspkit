// Implements [PROF-TESTING]. Shared native-capture assertions for the real VSIX host.
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import type { ProfileResult } from "../profiler-decorations";
import { buildFlamegraphHtml, flamegraphPanelState, openFlamegraphWebview } from "../profiler-flamegraph-html";
import { parseSpeedscope } from "../flame-model";
import { isRecord } from "../unknown-shape";
import { assertHardened } from "./webview-contract-helpers";

/** The captured result must attribute samples to real source files and carry both artifacts. */
export async function capturesAttributedSamples(result: ProfileResult): Promise<void> {
  assert.ok(result.totalSamples > 0);
  assert.ok(result.hotFunctions.some(fn => fn.file !== undefined && fn.file !== "" && fn.percentage > 0));
  const parsed = parseSpeedscope(await fs.promises.readFile(result.outputFile, "utf8"));
  assert.ok(parsed.ok, parsed.ok ? "" : parsed.error);
  assert.ok(result.cpuProfilePath !== undefined && result.cpuProfilePath !== "");
  const cpu: unknown = JSON.parse(await fs.promises.readFile(result.cpuProfilePath, "utf8"));
  assert.ok(isRecord(cpu) && Array.isArray(cpu.nodes) && cpu.nodes.length > 0 && Array.isArray(cpu.samples) && cpu.samples.length > 0);
}

/** The real host must mount both interactive views and the expected controls. */
export async function mountsTheInteractiveViews(result: ProfileResult): Promise<void> {
  openFlamegraphWebview(result);
  const panel = flamegraphPanelState();
  assert.ok(panel !== undefined);
  for (const id of ["flame", "btn-left", "btn-time", "fiber-select", "search", "btn-reset", "trace-actions", "fn-body", "line-body"]) {
    assert.ok(panel.html.includes(`id="${id}"`), `${id} is reachable in the real host`);
  }
  assertHardened(panel.html, "captured profile");
  const output = process.env.PROFILER_REVIEW_HTML;
  if (output !== undefined && output !== "") { await fs.promises.writeFile(output, buildFlamegraphHtml(result)); }
}
