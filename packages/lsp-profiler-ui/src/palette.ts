// Implements [PROF-UI-PALETTE]. Pure, validated host palette shared by every surface.
// framePaletteColor, paletteFrames and everything they call are embedded in
// the flame webview by Function.prototype.toString (flame-script.ts), so they
// stay self-contained declarations that call only each other.
import { isRecord } from "./unknown-shape";

export interface ProfilerPalette {
  readonly critical: string;
  readonly hot: string;
  readonly warm: string;
  readonly memory: string;
  readonly memoryHot: string;
  readonly memoryWarm?: string;
  readonly memoryCool?: string;
  readonly leak: string;
  readonly success: string;
}

export const DEFAULT_PALETTE: ProfilerPalette = {
  critical: "#e8500a", hot: "#f97316", warm: "#fbbf24",
  memory: "#c084fc", memoryHot: "#a78bfa", memoryWarm: "#8b5cf6", memoryCool: "#7c3aed", leak: "#f87171", success: "#34d399",
};

/** The CSS variable each palette colour sets, in every profiler view and on every live repaint. */
const PALETTE_CSS_VARS: readonly (readonly [keyof ProfilerPalette, string])[] = [
  ["critical", "--prof-critical"], ["hot", "--prof-hot"], ["warm", "--prof-warm"], ["memory", "--prof-mem-critical"],
  ["memoryHot", "--prof-mem-hot"], ["memoryWarm", "--prof-mem-warm"], ["memoryCool", "--prof-mem-cool"],
  ["leak", "--prof-mem-leak"], ["success", "--prof-success"],
];

/** A six-digit hex colour: the only override CSS can never be injected through. */
const HEX_COLOR = /^#[0-9a-f]{6}$/iu;

let readPalette: () => ProfilerPalette = () => DEFAULT_PALETTE;

/** Bind host configuration; each render reads the current palette. */
export function configurePalette(read: () => ProfilerPalette): void { readPalette = read; }
export function profilerPalette(): ProfilerPalette { return readPalette(); }

/** Reject CSS injection and invalid overrides, retaining the host default. */
export function resolvePalette(base: ProfilerPalette, overrides: unknown): ProfilerPalette {
  if (!isRecord(overrides)) { return base; }
  const pick = (key: keyof ProfilerPalette, fallback: string): string => {
    const value = overrides[key];
    return typeof value === "string" && HEX_COLOR.test(value) ? value : fallback;
  };
  return {
    critical: pick("critical", base.critical), hot: pick("hot", base.hot), warm: pick("warm", base.warm),
    memory: pick("memory", base.memory), memoryHot: pick("memoryHot", base.memoryHot),
    memoryWarm: pick("memoryWarm", base.memoryWarm ?? base.memoryHot), memoryCool: pick("memoryCool", base.memoryCool ?? base.memoryHot),
    leak: pick("leak", base.leak), success: pick("success", base.success),
  };
}

/**
 * Every palette CSS variable with its value, and `--prof-hot-ink`, the text
 * colour readable on the hot accent. A palette without a memory ramp ramps
 * from its own memory accent, never the default's.
 */
export function paletteVars(palette: ProfilerPalette = profilerPalette()): Record<string, string> {
  const valid = resolvePalette(DEFAULT_PALETTE, { ...palette,
    memoryWarm: palette.memoryWarm ?? palette.memoryHot, memoryCool: palette.memoryCool ?? palette.memoryHot });
  const vars = PALETTE_CSS_VARS.map(([key, name]): [string, string] => [name, valid[key] ?? valid.memoryHot]);
  return { ...Object.fromEntries(vars), "--prof-hot-ink": inkFor(valid.hot) };
}

/** The palette as CSS variables, with the same names in every profiler view. */
export function paletteCss(palette: ProfilerPalette = profilerPalette()): string {
  return `:root { ${Object.entries(paletteVars(palette)).map(([name, value]) => `${name}: ${value};`).join(" ")} }`;
}

const COLOR_CHANNEL_MAX = 255;
const LUMA_RED = 0.2126;
const LUMA_GREEN = 0.7152;
const LUMA_BLUE = 0.0722;
const LUMA_THRESHOLD = 0.5;
const RED_CHANNEL_OFFSET = 1;
const GREEN_CHANNEL_OFFSET = 3;
const BLUE_CHANNEL_OFFSET = 5;

/**
 * Text that stays readable on `fill`, a six-digit hex colour: dark ink on a
 * light fill, light ink on a dark one. Embedded in the flame webview, so it
 * calls nothing else.
 */
export function inkFor(fill: string): string {
  const channel = (at: number): number => Number.parseInt(fill.slice(at, at + 2), 16) / COLOR_CHANNEL_MAX;
  const luma = LUMA_RED * channel(RED_CHANNEL_OFFSET) + LUMA_GREEN * channel(GREEN_CHANNEL_OFFSET) + LUMA_BLUE * channel(BLUE_CHANNEL_OFFSET);
  return luma < LUMA_THRESHOLD ? "rgba(245, 245, 245, 0.95)" : "rgba(15, 15, 15, 0.9)";
}

/** The source classification a host that supplies none gets: a frame with a file is source. */
export function hasSourceFile(file: string): boolean {
  return file !== "";
}

/** A frame reduced to what decides its rank: its key, and where it came from. */
export interface FrameRank { key: string; i: number }

/**
 * The frame ordering policy: by key, then by original index.
 *
 * The index term is not decoration. Two frames can share a file AND a name --
 * one function inlined at two sites, or two runtime frames with no file at all
 * -- and a comparator that answers 0 for those hands their order to whatever
 * the engine does with elements it was told are equal. Rank is what picks a
 * frame's colour, so an order the engine decides is a colour the engine
 * decides, and the same profile stops looking the same twice.
 *
 * Exported because a sort's OUTPUT cannot tell this apart from a comparator
 * with no tie-break at all: `Array.prototype.sort` is required to be stable,
 * so equal elements come back in input order either way. The policy has to be
 * asserted directly or it is not pinned.
 */
export function byFrameKeyThenIndex(left: FrameRank, right: FrameRank): number {
  if (left.key < right.key) { return -1; }
  if (left.key > right.key) { return 1; }
  return left.i - right.i;
}

/** Each frame's rank by (file, name): frame identity, never array order, picks its colour. */
export function frameRanks(frames: readonly { file?: string; name: string }[]): number[] {
  const ranks = Array.from({ length: frames.length }, () => 0);
  const keyed = frames.map((frame, index) => ({ key: `${frame.file ?? ""}\u0000${frame.name}`, i: index }));
  for (const [rank, entry] of keyed.toSorted(byFrameKeyThenIndex).entries()) {
    ranks[entry.i] = rank;
  }
  return ranks;
}

const RAMP_SIZE = 3;

/** Stable frame ranks choose from the host ramp; unknown frames stay neutral. */
export function framePaletteColor(index: number, source: boolean, palette: ProfilerPalette): string {
  return source ? [palette.critical, palette.hot, palette.warm][index % RAMP_SIZE] : "#78818b";
}

const CPU_HEAT_HIGH = 20;
const CPU_HEAT_MEDIUM = 10;
const CPU_HEAT_LOW = 5;

/** CPU heat thresholds shared by canvas and source annotations. */
export function cpuHeatColor(pct: number, palette = profilerPalette()): string {
  if (pct >= CPU_HEAT_HIGH) { return palette.critical; }
  if (pct >= CPU_HEAT_MEDIUM) { return palette.hot; }
  if (pct >= CPU_HEAT_LOW) { return palette.warm; }
  return "#88888899";
}

/** Every frame coloured from the host ramp by its rank. */
export function paletteFrames<Frame extends { file: string; name: string; color: string; source?: boolean }>(frames: Frame[], palette: ProfilerPalette): Frame[] {
  const ranks = frameRanks(frames);
  return frames.map((frame, index) => ({ ...frame,
    color: framePaletteColor(ranks[index] ?? index, frame.source ?? hasSourceFile(frame.file), palette) }));
}
