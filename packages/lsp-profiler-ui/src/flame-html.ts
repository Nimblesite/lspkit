// Markup for the interactive sampled view ([PROF-UI-SAMPLED]) that the shared
// CPU results panel mounts: the header strip + fiber chips, the Left Heavy /
// Time Order toolbar, the canvas, the tooltip and the JSON data blob, styled by
// theme-aware --vscode-* variables. No vscode imports — unit tested as a string.

import type { FlameModel } from "./flame-model";
import { escapeHtml } from "./profiler-styles";
import { formatSummaryHeader, onCpuPct, type ProfileSummary } from "./sample-summary";

export { escapeHtml } from "./profiler-styles";

/** The final path segment (both separators), for compact locations. */
export function pathBasename(filePath: string): string {
  const parts = filePath.split(/[\\/]/);
  return parts[parts.length - 1] || filePath;
}

function chipsHtml(summary: ProfileSummary): string {
  // Only fibers that produced samples get chips: the speedscope export (and
  // therefore model.fibers) contains exactly those fibers, in the same row
  // order, so the filtered index is what setFiber() needs. Including
  // zero-sample rows would shift every later chip onto the wrong fiber.
  return summary.fibers
    .filter((fiber) => fiber.samples > 0)
    .map(
      (fiber, i) =>
        `<button class="chip" data-fiber="${i}">${escapeHtml(fiber.label)} · ` +
        `${summary.schedulingKnown === false ? `${fiber.samples} samples` : `${onCpuPct(fiber)}% on-CPU`}</button>`,
    )
    .join("");
}

function fiberOptionsHtml(model: FlameModel): string {
  return model.fibers
    .map(
      (fiber, i) => `<option value="${i}">${escapeHtml(fiber.name)}</option>`,
    )
    .join("");
}

// The webview stylesheet: VS Code theme tokens only — no external fonts.
export const FLAME_CSS = `
:root { color-scheme: light dark; }
* { box-sizing: border-box; }
body {
  margin: 0; padding: 10px 14px; font-size: 12px;
  font-family: var(--vscode-font-family, sans-serif);
  color: var(--vscode-foreground); background: var(--vscode-editor-background);
}
.header { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin-bottom: 8px; }
.stats { color: var(--vscode-descriptionForeground); }
.chip {
  border: 1px solid var(--vscode-panel-border, #5558); border-radius: 10px;
  background: var(--vscode-editorWidget-background); color: var(--vscode-foreground);
  padding: 1px 9px; cursor: pointer; font-size: 11px; font-family: inherit;
}
.chip.active { border-color: var(--vscode-focusBorder, #3794ff); }
.toolbar { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; margin-bottom: 8px; }
.toggle {
  border: 1px solid transparent; border-radius: 3px; padding: 2px 10px; cursor: pointer;
  background: var(--vscode-button-secondaryBackground, #3a3d41);
  color: var(--vscode-button-secondaryForeground, #ccc); font-family: inherit; font-size: 12px;
}
.toggle.active {
  background: var(--vscode-button-background, #0e639c);
  color: var(--vscode-button-foreground, #fff);
}
#fiber-select {
  background: var(--vscode-dropdown-background); color: var(--vscode-dropdown-foreground);
  border: 1px solid var(--vscode-dropdown-border, transparent); padding: 2px 4px; font-family: inherit;
}
#search {
  flex: 0 1 240px; padding: 2px 6px; font-family: inherit;
  background: var(--vscode-input-background); color: var(--vscode-input-foreground);
  border: 1px solid var(--vscode-input-border, transparent);
}
#flame-wrap { position: relative; border: 1px solid var(--vscode-panel-border, #5554); }
#flame { display: block; max-width: 100%; }
#tooltip {
  position: fixed; display: none; z-index: 10; pointer-events: none; max-width: 480px;
  padding: 6px 9px; font-size: 11px; border-radius: 3px;
  background: var(--vscode-editorHoverWidget-background, #252526);
  color: var(--vscode-editorHoverWidget-foreground, #ccc);
  border: 1px solid var(--vscode-editorHoverWidget-border, #454545);
}
.tt-name { font-weight: 600; }
.tt-loc, .tt-nums { color: var(--vscode-descriptionForeground); }
`;

function embedJson(model: FlameModel, summary: ProfileSummary): string {
  // <-escape so "</script>" inside frame names can never close the blob.
  return JSON.stringify({ model, summary }).replace(/</g, "\\u003c");
}

/** The interactive sampled view, mounted in the shared CPU results panel. */
export function flameBody(model: FlameModel, summary: ProfileSummary): string {
  return `<div class="header"><span class="stats">${escapeHtml(formatSummaryHeader(summary))}</span>${chipsHtml(summary)}</div>
<div class="toolbar">
<button id="btn-left" class="toggle active">Left Heavy</button>
<button id="btn-time" class="toggle">Time Order</button>
<select id="fiber-select">${fiberOptionsHtml(model)}</select>
<input id="search" type="text" placeholder="Search functions (Esc clears)">
<span id="match-label" class="stats"></span>
<span id="hidden-label" class="stats"></span>
<button id="btn-reset" class="toggle">Reset zoom (0)</button>
</div>
<div id="flame-wrap"><canvas id="flame"></canvas></div>
<div id="tooltip"></div>
<script type="application/json" id="flame-data">${embedJson(model, summary)}</script>`;
}
