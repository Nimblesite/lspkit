// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * CPU profile results webview for the profiler.
 *
 * Owns the results panel end to end: the interactive flame graph hero (the
 * inferno SVG the LSP exports on every stop, [PROF-EXPORT-FLAMEGRAPH]), summary
 * cards, hot function/line tables with click-to-source navigation, and the
 * result-landing flow that never dead-ends the user (#145): the panel is the
 * primary view on stop, with the raw V8 trace one click away.
 *
 * Panel lifecycle, CSP, and safe data embedding come from the shared webview
 * host ([PROF-UI-WEBVIEW], profiler-webview.ts).
 */

import { sampledParts, sampledView } from "./sampled-view";
import { flamegraphHeroHtml, loadFlamegraphSvgDataUri } from "./flamegraph-artifact";
import { brand, cmd } from "./ids";
import * as vscode from "vscode";
import { Logger } from "./logger";
import { errorText } from "./failure";
import { openNativeTraceViewer, openSpeedscopeImport, revealTraceFile, traceFileFor } from "./trace-actions";
import type { ProfileResult } from "./profiler-decorations";
import {
  PROFILER_CSS_CARDS,
  PROFILER_CSS_HEADING,
  PROFILER_CSS_RESET,
  PROFILER_CSS_TABLE,
  PROFILER_CSS_VARS,
  PROFILER_JS_UTILS,
  brandHeading,
} from "./profiler-styles";
import {
  SingletonWebviewPanel,
  buildWebviewDocument,
  embedJson,
  handleSourceNavigation,
} from "./profiler-webview";
import type { WebviewMessage } from "./profiler-webview";

export { loadFlamegraphSvgDataUri } from "./flamegraph-artifact";

// Implements [PROF-CPU-START]. See docs/specs/LSP-PROFILER-SPEC.md
// ── Result presentation (#145) ─────────────────────────────────────────────

/** Completion-toast actions that always reach a working view (#145). */
const OPEN_NATIVE_TRACE = "Open Trace in VS Code Viewer";
const REVEAL_TRACE = "Reveal Trace File";

// Implements [PROF-CPU-START]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Whether a completed profile has nothing worth showing — no hot functions and
 * no hot lines (#145).
 *
 * Raw sample count is the wrong signal: a sub-tick program (e.g.
 * `debug_demo.py` ≈ 1 ms) finishes before its work can be sampled, yet the
 * session keeps sampling the idle/exiting interpreter — so the result can carry
 * dozens of samples that resolve to zero user-code attribution. A sampling
 * profiler at 100 Hz cannot fix this by raising the rate; deterministic
 * profiling can (Phase 2). When there is no attribution, say so honestly instead
 * of presenting an empty flame chart/heat map as a result.
 */
export function profileHasNoUsableData(
  result: Pick<ProfileResult, "hotFunctions" | "hotLines">,
): boolean {
  return result.hotFunctions.length === 0 && result.hotLines.length === 0;
}

/**
 * Land a completed CPU profile on a viewable result ([PROF-CPU-START], #145).
 *
 * The self-contained results panel (summary cards, flame graph hero, navigable
 * hot-function/line tables) opens immediately as the primary view: it always
 * renders, while VS Code's built-in `.cpuprofile` viewer lands on a raw
 * self/total-time table that reads as a wall of numbers until the user finds
 * its flame icon. The native trace stays one deliberate click away — the
 * completion toast's "Open Trace in VS Code Viewer" action, the panel's own
 * button, or the `${brand()}: Show Profile Results` re-entry command — so the
 * user is never stranded and never dumped somewhere confusing.
 */
export async function presentProfileResult(result: ProfileResult): Promise<void> {
  if (profileHasNoUsableData(result)) {
    presentNoUsableData(result);
    return;
  }
  await openFlamegraphWebview(result);
  Logger.info(`Profiling stopped: ${String(result.totalSamples)} samples, ${result.duration.toFixed(1)}s, output: ${result.outputFile}`);
  // A failed/refused export is never silent — even when there is still data to
  // show, surface why an artifact is missing ([PROF-EXPORT-VALIDATE]).
  if (result.exportError !== undefined && result.exportError !== "") {
    void vscode.window.showWarningMessage(`${brand()}: Some profile artifacts could not be exported — ${result.exportError}`);
  }
  // Fire-and-forget: a notification carrying action buttons is sticky (VS Code
  // does not auto-dismiss it), so awaiting it would block the stop handler until
  // the user clicks. Its actions open their views when chosen; a rejection is logged.
  void offerProfileResultActionsSafely(result);
}

/**
 * A profile with no attribution (#145, [PROF-CPU-START]). Be honest —
 * don't present an empty flame chart/heat map as a real result, and don't
 * promise a "higher sample rate" that provably cannot help a sub-tick run.
 * Fire-and-forget (a notification is sticky if it carries a button, but even
 * plain ones must not block the stop handler).
 */
function presentNoUsableData(result: ProfileResult): void {
  const detail =
    result.exportError !== undefined && result.exportError !== "" ? ` (${result.exportError})` : "";
  if (result.totalSamples === 0) {
    void vscode.window.showWarningMessage(
      `${brand()}: Profiling captured no samples — the program finished before sampling began${detail}. ` +
        "Sampling needs a longer-running target; short scripts need precise (deterministic) profiling.",
    );
    return;
  }
  void vscode.window.showWarningMessage(
    `${brand()}: Captured ${String(result.totalSamples)} sample${result.totalSamples === 1 ? "" : "s"}, but none ` +
      "landed in your code — the program ran its work too briefly to profile by sampling. " +
      "Short scripts need precise (deterministic) profiling.",
  );
}

/** Completion toast that always offers a path to the raw trace (#145). */
async function offerProfileResultActions(result: ProfileResult): Promise<void> {
  const choice = await vscode.window.showInformationMessage(
    `${brand()}: Profile complete — ${String(result.totalSamples)} samples in ${result.duration.toFixed(1)}s`,
    OPEN_NATIVE_TRACE,
    REVEAL_TRACE,
  );
  if (choice === OPEN_NATIVE_TRACE) {
    await openNativeTraceViewer(result.cpuProfilePath ?? "", traceFileFor(result));
  } else if (choice === REVEAL_TRACE) {
    await revealTraceFile(traceFileFor(result));
  }
}

/** Offer the completion actions, turning a failed action into a logged warning. */
async function offerProfileResultActionsSafely(result: ProfileResult): Promise<void> {
  try {
    await offerProfileResultActions(result);
  } catch (error: unknown) {
    Logger.warn(`Profile completion action failed: ${errorText(error)}`);
  }
}

// ── Webview panel ─────────────────────────────────────────────────────────

/** Route a message posted from the results webview; a host that keeps its own results panel routes through it too. */
export function handleFlamegraphMessage(msg: WebviewMessage): void {
  if (handleSourceNavigation(msg.type === "select" ? { ...msg, type: "navigateToSource" } : msg)) {
    return;
  }
  const file = msg.file ?? "";
  if (file === "") { return; }
  if (msg.type === "openSpeedscope") {
    void openSpeedscopeImport(file);
  } else if (msg.type === "openCpuProfile") {
    // The panel's "Open Trace in VS Code Viewer" button — the on-demand path to
    // the built-in `.cpuprofile` table view ([PROF-CPU-START]).
    void openNativeTraceViewer(file, file);
  } else if (msg.type === "openFlamegraphSvg") {
    // The inferno SVG carries its own zoom/search interactivity, which the
    // CSP-locked webview intentionally does not run — open it externally where
    // its script works ([PROF-EXPORT-FLAMEGRAPH]).
    void vscode.env.openExternal(vscode.Uri.file(file));
  }
}

const flamegraphPanel = new SingletonWebviewPanel(() => cmd("flamegraph"), handleFlamegraphMessage);

/** Open (or reveal) the profile results panel beside the source. */
export async function openFlamegraphWebview(result: ProfileResult): Promise<void> {
  flamegraphPanel.show(`${brand()} Profiler`, await buildFlamegraphHtml(result));
}

/** The open results panel's view type, title and HTML (e2e seam). */
export function flamegraphPanelState(): { readonly viewType: string; readonly title: string; readonly html: string } | undefined {
  const described = flamegraphPanel.describe();
  return described === undefined ? undefined : { ...described, html: flamegraphPanel.html() ?? "" };
}

/**
 * Test seam: is the self-contained results panel currently open? Lets the
 * run→profile→view e2e assert the user reaches a working flame chart rather
 * than dead-ending on the built-in viewer's error ([PROF-CPU-START]).
 */
export function flamegraphPanelOpen(): boolean {
  return flamegraphPanel.isOpen();
}

/** Close and forget the panel (extension teardown). */
export function disposeFlamegraphPanel(): void {
  flamegraphPanel.dispose();
}

// ── Document assembly ─────────────────────────────────────────────────────

/** The results panel's layout: fixed-width tables, the sampled view's accents, narrow panels and reduced motion. */
const FLAMEGRAPH_LAYOUT_CSS = `
    body { padding: 20px; }
    .summary-cards { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); max-width: 720px; }
    .card { min-width: 0; padding: 12px; }
    .data-table { table-layout: fixed; }
    .data-table td, .data-table th { overflow-wrap: anywhere; }
    .data-table th:first-child { width: 27%; }
    .data-table th:nth-child(2) { width: 37%; }
    .data-table .bar-cell { width: auto; overflow: hidden; }
    .bar { max-width: 100%; }
    .toggle.active { background: var(--prof-hot); color: var(--prof-hot-ink); }
    .chip.active { border-color: var(--prof-hot); }
    #search:focus { outline: 1px solid var(--prof-hot); outline-offset: 1px; }
    @media (max-width: 600px) {
      body { padding: 12px; }
      .summary-cards { gap: 8px; grid-template-columns: repeat(2, minmax(0, 1fr)); }
      .data-table td { padding: 7px 4px; font-size: 11px; }
      .data-table th { padding: 6px 4px; font-size: 10px; }
      .data-table th:last-child, .data-table .bar-cell { display: none; }
    }
    @media (prefers-reduced-motion: reduce) { * { transition: none !important; } }`;

/** Flamegraph-specific CSS on top of the shared profiler design system. */
const FLAMEGRAPH_CSS = `${PROFILER_CSS_VARS}${PROFILER_CSS_RESET}${PROFILER_CSS_HEADING}${PROFILER_CSS_CARDS}${PROFILER_CSS_TABLE}${FLAMEGRAPH_LAYOUT_CSS}
    h1 .accent { color: var(--prof-critical); }
    .data-table .pct { font-weight: 500; min-width: 56px; text-align: right; }
    .bar.critical { background: var(--prof-critical); }
    .bar.hot { background: var(--prof-hot); }
    .bar.warm { background: var(--prof-warm); }
    .bar.cool { background: var(--prof-cool); }
    .file-link { color: var(--prof-text-secondary); font-size: 12px; }
    .file-link:hover { color: var(--prof-text); text-decoration: underline; }
    .flame-hero {
      background: var(--prof-surface); border: 1px solid var(--prof-border);
      border-radius: 8px; padding: 12px; margin-bottom: 8px;
    }
    .flame-hero img { display: block; width: 100%; height: auto; border-radius: 4px; }
    .action-link {
      display: inline-block; margin-top: 12px; padding: 8px 16px;
      background: var(--prof-bg); border: 1px solid var(--prof-border);
      border-radius: 6px; color: var(--prof-text);
      font-size: 13px; cursor: pointer; font-family: inherit;
    }
    .action-link:hover { border-color: var(--prof-critical); color: var(--prof-critical); }
    .action-link + .action-link { margin-left: 8px; }`;

/** The run's totals, filled in by the script. */
const SUMMARY_CARDS_HTML = `
  <div class="summary-cards">
    <div class="card"><div class="label">Samples</div><div class="value" id="total-samples">0</div></div>
    <div class="card"><div class="label">Duration</div><div class="value" id="duration">0s</div></div>
    <div class="card"><div class="label">Functions</div><div class="value" id="fn-count">0</div></div>
    <div class="card"><div class="label">Hot Lines</div><div class="value" id="line-count">0</div></div>
  </div>`;

/** The hot function and hot line tables, and the trace actions under them. */
const HOT_TABLES_HTML = `
  <h2>Hot Functions</h2>
  <table class="data-table">
    <thead><tr><th>Function</th><th>Location</th><th>Total %</th><th>Self %</th><th></th></tr></thead>
    <tbody id="fn-body"></tbody>
  </table>
  <h2>Hot Lines</h2>
  <table class="data-table">
    <thead><tr><th>Location</th><th>%</th><th>Samples</th><th></th></tr></thead>
    <tbody id="line-body"></tbody>
  </table>
  <div id="trace-actions"></div>`;

/** Build the body HTML with the flame graph hero, summary cards and tables. */
function flamegraphBody(svgDataUri: string | undefined, sampledBody = ""): string {
  return `
  ${brandHeading(brand(), "PROFILER")}${SUMMARY_CARDS_HTML}${sampledBody}${flamegraphHeroHtml(svgDataUri)}${HOT_TABLES_HTML}`;
}

/** The result as script constants; profiled-program strings are embedded inert ([PROF-UI-WEBVIEW]). */
function flamegraphDataScript(result: ProfileResult): string {
  return `
    const vscode = globalThis.profilerVsCodeApi || (globalThis.profilerVsCodeApi = acquireVsCodeApi());
    const hotFunctions = ${embedJson(result.hotFunctions)};
    const hotLines = ${embedJson(result.hotLines)};
    const totalSamples = ${String(result.totalSamples)};
    const duration = ${String(result.duration)};
    const outputFile = ${embedJson(result.outputFile)};
    const flamegraphFile = ${embedJson(result.flamegraphPath ?? "")};
    const cpuProfileFile = ${embedJson(result.cpuProfilePath ?? "")};`;
}

/** The summary cards, the hero's open button and the heat classes the tables use. */
const SUMMARY_SCRIPT = `
    document.getElementById('total-samples').textContent = String(totalSamples);
    document.getElementById('duration').textContent = duration < 1 ? (duration * 1000).toFixed(0) + 'ms' : duration < 60 ? duration.toFixed(1) + 's' : (duration / 60).toFixed(1) + 'm';
    document.getElementById('fn-count').textContent = String(hotFunctions.length);
    document.getElementById('line-count').textContent = String(hotLines.length);
    const openSvgButton = document.getElementById('open-flame-svg');
    if (openSvgButton) {
      openSvgButton.onclick = () => vscode.postMessage({ type: 'openFlamegraphSvg', file: flamegraphFile });
    }
    function heatClass(pct) {
      if (pct >= 20) return 'critical';
      if (pct >= 10) return 'hot';
      if (pct >= 5) return 'warm';
      return 'cool';
    }`;

/** The hot function and hot line rows, each navigating to its source. */
const TABLES_SCRIPT = `
    const fnBody = document.getElementById('fn-body');
    for (const fn of hotFunctions) {
      const tr = document.createElement('tr');
      // A frame the runtime could not place (a framework method) has no file: it
      // is listed by name, with no link to nowhere.
      const placed = fn.file !== undefined;
      if (placed) {
        tr.onclick = () => vscode.postMessage({ type: 'navigateToSource', file: fn.file, line: fn.line });
      }
      // fn.name/fn.file come from the profiled (possibly third-party) program —
      // escape before innerHTML. This also renders synthetic frames like
      // <module>/<lambda> literally instead of as broken HTML elements.
      tr.innerHTML = [
        '<td>' + escapeHtml(fn.name) + '</td>',
        placed ? '<td class="file-link">' + escapeHtml(basename(fn.file)) + ':' + fn.line + '</td>' : '<td></td>',
        '<td class="pct">' + fn.percentage.toFixed(1) + '%</td>',
        '<td class="pct">' + fn.selfPercentage.toFixed(1) + '%</td>',
        '<td class="bar-cell"><div class="bar ' + heatClass(fn.percentage) + '" style="width:' + Math.max(4, fn.percentage * 2) + 'px"></div></td>',
      ].join('');
      fnBody.appendChild(tr);
    }
    const lineBody = document.getElementById('line-body');
    for (const line of hotLines) {
      const tr = document.createElement('tr');
      tr.onclick = () => vscode.postMessage({ type: 'navigateToSource', file: line.file, line: line.line });
      tr.innerHTML = [
        '<td class="file-link">' + escapeHtml(basename(line.file)) + ':' + line.line + '</td>',
        '<td class="pct">' + line.percentage.toFixed(1) + '%</td>',
        '<td>' + line.samples + '</td>',
        '<td class="bar-cell"><div class="bar ' + heatClass(line.percentage) + '" style="width:' + Math.max(4, line.percentage * 2) + 'px"></div></td>',
      ].join('');
      lineBody.appendChild(tr);
    }`;

/** The raw trace and speedscope buttons, offered only for artifacts that exist. */
const TRACE_ACTIONS_SCRIPT = `
    const section = document.getElementById('trace-actions');
    function traceAction(label, title, type, file) {
      const button = document.createElement('button');
      button.className = 'action-link';
      button.textContent = label;
      button.title = title;
      button.onclick = () => { vscode.postMessage({ type, file }); };
      section.appendChild(button);
    }
    if (cpuProfileFile) {
      traceAction('Open Trace in VS Code Viewer', 'Open the raw .cpuprofile in the built-in trace viewer (self/total time table)', 'openCpuProfile', cpuProfileFile);
    }
    if (outputFile) {
      traceAction('Open in Speedscope (external)', 'Open speedscope.app with the profile loaded automatically', 'openSpeedscope', outputFile);
    }`;

/** Build the complete results-panel HTML for the profiler webview. */
export async function buildFlamegraphHtml(result: ProfileResult): Promise<string> {
  const [svgDataUri, view] = await Promise.all([loadFlamegraphSvgDataUri(result.flamegraphPath), sampledView(result)]);
  const sampled = sampledParts(view);
  return buildWebviewDocument({
    title: `${brand()} Profiler`,
    css: `${sampled.css}${FLAMEGRAPH_CSS}`,
    body: flamegraphBody(svgDataUri, sampled.body),
    script: `${PROFILER_JS_UTILS}${flamegraphDataScript(result)}${SUMMARY_SCRIPT}${TABLES_SCRIPT}${TRACE_ACTIONS_SCRIPT}${sampled.script}`,
  });
}
