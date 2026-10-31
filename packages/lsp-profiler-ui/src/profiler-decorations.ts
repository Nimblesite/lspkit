// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Inline heat map decorations for CPU profiling results.
 *
 * After profiling completes, hot lines get colored decorations in the
 * editor gutter and after-line text showing CPU percentage and sample
 * count. Uses the host palette ([PROF-UI-PALETTE]):
 *
 *   Critical (>20%): critical
 *   Hot (10-20%):    hot
 *   Warm (5-10%):    warm
 *   Cool (1-5%):     #4a5468  — muted slate, in every host
 */

import { profilerPalette } from "./palette";
import type { SampledView } from "./sampled-view";
import * as vscode from "vscode";
import { configSection } from "./ids";
import { Logger } from "./logger";
import { groupByFile, rowAndEditorKeyer } from "./editor-path-key";

// ── Types ─────────────────────────────────────────────────────────────────

/** Per-line profiling data from the LSP server. */
export interface ProfileHotLine {
  file: string;
  line: number;
  samples: number;
  percentage: number;
}

/** Per-function profiling data from the LSP server. */
export interface ProfileHotFunction {
  name: string;
  /** Source path; absent for a frame the runtime could not place, such as a framework method ([PROF-DOTNET-CPU-SYMBOLS]). */
  file?: string;
  /** 1-based definition line; present exactly when `file` is. */
  line?: number;
  samples: number;
  percentage: number;
  selfPercentage: number;
}

/** Profile result returned by `<prefix>.lspProfiler.cpu.stop` or `/snapshot`. */
export interface ProfileResult {
  sampled?: SampledView;
  sessionId: string;
  duration: number;
  totalSamples: number;
  outputFile: string;
  /** Path to the V8 `.cpuprofile` for VS Code's built-in profile viewer. */
  cpuProfilePath?: string;
  /**
   * Path to the self-contained interactive flame graph SVG the LSP always
   * attempts to export ([PROF-EXPORT-FLAMEGRAPH]). Embedded as the hero of the
   * results webview and openable externally for full zoom/search.
   */
  flamegraphPath?: string;
  /**
   * Why an export was refused or failed ([PROF-EXPORT-VALIDATE]) — e.g.
   * a zero-sample run. Surfaced to the user; never silent (#145).
   */
  exportError?: string;
  hotFunctions: ProfileHotFunction[];
  hotLines: ProfileHotLine[];
}

// ── Heat levels ───────────────────────────────────────────────────────────

interface HeatLevel {
  minPct: number;
  color: string;
  barChar: string;
  barMaxLen: number;
}

/** Minimum percentage for "critical" heat classification. */
const HEAT_CRITICAL_PCT = 20;
/** Minimum percentage for "hot" heat classification. */
const HEAT_HOT_PCT = 10;
/** Minimum percentage for "warm" heat classification. */
const HEAT_WARM_PCT = 5;

/** Max bar length for critical heat level. */
const BAR_LEN_CRITICAL = 10;
/** Max bar length for hot heat level. */
const BAR_LEN_HOT = 8;
/** Max bar length for warm heat level. */
const BAR_LEN_WARM = 6;
/** Max bar length for cool heat level. */
const BAR_LEN_COOL = 4;

const heatLevels = (): HeatLevel[] => [
  { minPct: HEAT_CRITICAL_PCT, color: profilerPalette().critical, barChar: "\u2588", barMaxLen: BAR_LEN_CRITICAL },
  { minPct: HEAT_HOT_PCT, color: profilerPalette().hot, barChar: "\u2588", barMaxLen: BAR_LEN_HOT },
  { minPct: HEAT_WARM_PCT, color: profilerPalette().warm, barChar: "\u2588", barMaxLen: BAR_LEN_WARM },
  { minPct: 1, color: "#4a5468", barChar: "\u2588", barMaxLen: BAR_LEN_COOL },
];

function heatLevelFor(pct: number): HeatLevel | undefined {
  return heatLevels().find((level) => pct >= level.minPct);
}

// ── Applied-decoration ledger ─────────────────────────────────────────────
// Implements [PROF-UI-HEATMAP] observability: `setDecorations` is
// write-only in the VS Code API, so the heat map records what it applied and
// exposes it for e2e assertions (per the VSIX internal-state testing rule).

/** One heat decoration as applied to an open editor. */
export interface AppliedDecoration {
  readonly file: string;
  /** 1-based line the decoration landed on. */
  readonly line: number;
  /** Palette color used (the host palette's `critical` for a critical line). */
  readonly color: string;
  /** The after-line text shown to the user (bar + % + samples). */
  readonly contentText: string;
}

let appliedDecorations: AppliedDecoration[] = [];

/** The heat-map decorations currently applied across visible editors. */
export function appliedProfileDecorations(): readonly AppliedDecoration[] {
  return appliedDecorations;
}

/** Record everything painted for one file, for the applied ledger. */
export function recordApplied(
  ledger: AppliedDecoration[],
  file: string,
  paintedByColor: ReadonlyMap<string, readonly vscode.DecorationOptions[]>,
): void {
  for (const [color, options] of paintedByColor) {
    for (const option of options) {
      ledger.push({
        file,
        line: option.range.start.line + 1,
        color,
        contentText: option.renderOptions?.after?.contentText ?? "",
      });
    }
  }
}

// ── Decoration types (created lazily, one per heat level) ─────────────────

const decorationTypes = new Map<string, vscode.TextEditorDecorationType>();

function getDecorationTypeForColor(color: string): vscode.TextEditorDecorationType {
  let existing = decorationTypes.get(color);
  if (existing !== undefined) { return existing; }

  existing = vscode.window.createTextEditorDecorationType({
    overviewRulerColor: color,
    overviewRulerLane: vscode.OverviewRulerLane.Right,
    gutterIconSize: "contain",
    isWholeLine: false,
    after: {
      margin: "0 0 0 2em",
      fontStyle: "normal",
      fontWeight: "500",
    },
  });
  decorationTypes.set(color, existing);
  return existing;
}

// ── Public API ────────────────────────────────────────────────────────────

/**
 * Apply inline heat map decorations to all visible editors based on
 * profiling results.
 */

/** Add a decoration option to the color-keyed options map. */
function addDecorationOption(
  optionsByColor: Map<string, vscode.DecorationOptions[]>,
  color: string,
  option: vscode.DecorationOptions,
): void {
  const existing = optionsByColor.get(color);
  if (existing === undefined) {
    optionsByColor.set(color, [option]);
  } else {
    existing.push(option);
  }
}

/** Decoration options grouped by heat colour: one decoration type per colour. */
type OptionsByColor = Map<string, vscode.DecorationOptions[]>;

/** The line, text and colour of one after-line heat hint. */
interface LineHint {
  readonly line: number;
  readonly text: string;
  readonly color: string;
}

/** Paint `hint.text` after source line `hint.line` (1-based) in `hint.color`, when the line exists. */
function addLineHint(editor: vscode.TextEditor, optionsByColor: OptionsByColor, hint: LineHint): void {
  const lineIdx = hint.line - 1;
  if (lineIdx < 0 || lineIdx >= editor.document.lineCount) { return; }
  const range = new vscode.Range(
    new vscode.Position(lineIdx, 0),
    new vscode.Position(lineIdx, editor.document.lineAt(lineIdx).text.length),
  );
  addDecorationOption(optionsByColor, hint.color, { range, renderOptions: { after: { contentText: hint.text, color: hint.color } } });
}

/** Build line-level heat decoration options for a single editor. */
function buildLineDecorations(editor: vscode.TextEditor, lines: ProfileHotLine[], optionsByColor: OptionsByColor): void {
  for (const line of lines) {
    const level = heatLevelFor(line.percentage);
    if (level !== undefined) {
      const barLen = Math.ceil((line.percentage / level.minPct) * (level.barMaxLen / 2));
      const bar = level.barChar.repeat(Math.min(barLen, level.barMaxLen));
      addLineHint(editor, optionsByColor, { line: line.line, text: ` ${bar} ${line.percentage.toFixed(1)}% (${String(line.samples)} samples)`, color: level.color });
    }
  }
}

/** A hot function the runtime placed in source, so the heat map can paint it. */
type LocatedHotFunction = ProfileHotFunction & { file: string; line: number };

function isLocated(func: ProfileHotFunction): func is LocatedHotFunction {
  return func.file !== undefined && func.line !== undefined;
}

/** Build function-level heat decoration options for a single editor; a line's own heat takes its place. */
function buildFuncDecorations(
  editor: vscode.TextEditor,
  funcs: LocatedHotFunction[],
  ctx: { lines: ProfileHotLine[] | undefined; optionsByColor: OptionsByColor },
): void {
  for (const func of funcs) {
    const level = heatLevelFor(func.percentage);
    if (level !== undefined && ctx.lines?.some((line) => line.line === func.line) !== true) {
      const text = ` ${func.name} \u2014 ${func.percentage.toFixed(1)}% CPU (${func.selfPercentage.toFixed(1)}% self)`;
      addLineHint(editor, ctx.optionsByColor, { line: func.line, text, color: level.color });
    }
  }
}

/** The result's hot lines and located hot functions, keyed by editor path. */
interface HeatByFile {
  readonly keyOf: (file: string) => string;
  readonly lines: Map<string, ProfileHotLine[]>;
  readonly funcs: Map<string, LocatedHotFunction[]>;
}

/** Group the result's located heat by the key of each file it names. */
async function heatByFile(result: ProfileResult, editors: readonly vscode.TextEditor[]): Promise<HeatByFile> {
  // An unlocated frame stays in the results table but paints no line.
  const located = result.hotFunctions.filter(isLocated);
  const keyOf = await rowAndEditorKeyer([...result.hotLines, ...located], editors);
  return { keyOf, lines: groupByFile(result.hotLines, keyOf), funcs: groupByFile(located, keyOf) };
}

export async function applyProfileDecorations(result: ProfileResult): Promise<void> {
  if (!vscode.workspace.getConfiguration(configSection()).get<boolean>("profiler.showInlineHeatMap", true)) {
    Logger.info("Profiler heat map disabled — skipping decorations");
    return;
  }
  clearProfileDecorations();
  const editors = vscode.window.visibleTextEditors;
  const heat = await heatByFile(result, editors);
  for (const editor of editors) { decorateEditor(editor, heat); }
  // Count what was PAINTED, not what the profile held: a profile full of hot
  // lines that matches no open editor used to log exactly like a successful
  // pass, which is how the heat map stayed silently blank on Windows.
  Logger.info(
    `Profile decorations applied: ${String(appliedDecorations.length)} across ${String(editors.length)} visible editor(s)`
    + ` from ${String(result.hotLines.length)} hot lines, ${String(result.hotFunctions.length)} hot functions`,
  );
}

/** Paint one editor's heat and record what was painted. */
function decorateEditor(editor: vscode.TextEditor, heat: HeatByFile): void {
  const filePath = editor.document.uri.fsPath;
  const optionsByColor = buildEditorDecorations(editor, heat);
  applyDecorationsByColor(editor, optionsByColor);
  recordApplied(appliedDecorations, filePath, optionsByColor);
}

/** Build the decoration options this editor will receive. */
function buildEditorDecorations(editor: vscode.TextEditor, heat: HeatByFile): OptionsByColor {
  const key = heat.keyOf(editor.document.uri.fsPath);
  const optionsByColor: OptionsByColor = new Map();
  const lines = heat.lines.get(key);
  if (lines !== undefined) { buildLineDecorations(editor, lines, optionsByColor); }
  const funcs = heat.funcs.get(key);
  if (funcs !== undefined) { buildFuncDecorations(editor, funcs, { lines, optionsByColor }); }
  return optionsByColor;
}

/** Apply one decoration type per colour to the editor. */
function applyDecorationsByColor(editor: vscode.TextEditor, optionsByColor: OptionsByColor): void {
  for (const [color, options] of optionsByColor) {
    editor.setDecorations(getDecorationTypeForColor(color), options);
  }
}

/** Clear all profiling decorations from visible editors. */
export function clearProfileDecorations(): void {
  for (const decorationType of decorationTypes.values()) {
    for (const editor of vscode.window.visibleTextEditors) {
      editor.setDecorations(decorationType, []);
    }
  }
  appliedDecorations = [];
}

/** Dispose all decoration types (call on extension deactivate). */
export function disposeProfileDecorations(): void {
  for (const decorationType of decorationTypes.values()) {
    decorationType.dispose();
  }
  decorationTypes.clear();
  appliedDecorations = [];
}
