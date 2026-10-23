// Implements [PROF-UI-PALETTE]. Pure, validated host palette shared by every surface.
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
let readPalette: () => ProfilerPalette = () => DEFAULT_PALETTE;

/** Bind host configuration; each render reads the current palette. */
export function configurePalette(read: () => ProfilerPalette): void { readPalette = read; }
export function profilerPalette(): ProfilerPalette { return readPalette(); }

/** Reject CSS injection and invalid overrides, retaining the host default. */
export function resolvePalette(base: ProfilerPalette, overrides: unknown): ProfilerPalette {
  if (typeof overrides !== "object" || overrides === null) return base;
  return Object.fromEntries(Object.entries({ memoryWarm: base.memoryHot, memoryCool: base.memoryHot, ...base }).map(([key, fallback]) => {
    const value: unknown = Reflect.get(overrides, key);
    return [key, typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value) ? value : fallback];
  })) as unknown as ProfilerPalette;
}

/** CSS variables have the same names in every profiler view. */
export function paletteCss(palette: ProfilerPalette = profilerPalette()): string {
  const valid = resolvePalette(DEFAULT_PALETTE, palette);
  return `:root { --prof-critical: ${valid.critical}; --prof-hot: ${valid.hot};
    --prof-warm: ${valid.warm}; --prof-mem-critical: ${valid.memory};
    --prof-mem-hot: ${valid.memoryHot}; --prof-mem-leak: ${valid.leak};
    --prof-success: ${valid.success}; }`;
}

/** Stable frame ranks choose from the host ramp; unknown frames stay neutral. */
export function framePaletteColor(index: number, source: boolean, palette: ProfilerPalette): string {
  return source ? [palette.critical, palette.hot, palette.warm][index % 3] : "#78818b";
}

/** CPU heat thresholds shared by canvas and source annotations. */
export function cpuHeatColor(pct: number, palette = profilerPalette()): string {
  return pct >= 20 ? palette.critical : pct >= 10 ? palette.hot : pct >= 5 ? palette.warm : "#88888899";
}

/** Frame identity determines color, independently of array ordering. */
export function paletteFrames<T extends { file: string; name: string; color: string; source?: boolean }>(frames: T[], palette: ProfilerPalette): T[] {
  const order = frames.map((frame, index) => ({ frame, index })).sort((a, b) =>
    a.frame.file < b.frame.file ? -1 : a.frame.file > b.frame.file ? 1 :
    a.frame.name < b.frame.name ? -1 : a.frame.name > b.frame.name ? 1 : a.index - b.index);
  const ranks = new Map(order.map(({ index }, rank) => [index, rank]));
  return frames.map((frame, index) => ({ ...frame,
    color: framePaletteColor(ranks.get(index) ?? index, frame.source ?? frame.file !== "", palette) }));
}
