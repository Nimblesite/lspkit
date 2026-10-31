// Pure helpers shared between the extension host and the flame-graph webview
// ([PROF-UI-SAMPLED]). FLAME_SCRIPT embeds these functions' COMPILED SOURCE
// via Function.prototype.toString, so the exact code the webview executes is
// type-checked and unit-tested here. Rules for anything in this file: no
// imports, no captured module state, no TS-only syntax that erases to
// different behavior — each function must be a self-contained declaration
// that serializes cleanly (target es2020; the webview is modern Chromium).

/**
 * Fit `name` into `avail` pixels by carving characters out of the middle and
 * inserting an ellipsis, using the caller's text `measure` (canvas
 * measureText in the webview, an injected fake in tests). Returns the widest
 * candidate that fits, or a lone ellipsis when nothing does.
 */
export function fitLabelText(
  name: string,
  avail: number,
  measure: (text: string) => number,
): string {
  // The fraction of the remaining name each mid-ellipsis step carves out.
  const KEEP_SHRINK_RATIO = 0.15;
  if (measure(name) <= avail) { return name; }
  for (let keep = name.length; keep > 2; ) {
    keep -= Math.max(1, Math.ceil(keep * KEEP_SHRINK_RATIO));
    const half = Math.floor(keep / 2);
    const candidate = `${name.slice(0, half)}…${name.slice(name.length - (keep - half))}`;
    if (measure(candidate) <= avail) { return candidate; }
  }
  return "…";
}

/** Case-insensitive substring match over frame names → set of frame indexes. */
export function searchMatches(frames: readonly { name: string }[], query: string): Set<number> {
  const needle = query.trim().toLowerCase();
  if (needle === "") { return new Set(); }
  return new Set(frames.flatMap((frame, i) => (frame.name.toLowerCase().includes(needle) ? [i] : [])));
}

/**
 * The share of the profile `slabs` cover, each instant counted once. Flame
 * slabs nest or are disjoint, so this is the sum of the outermost matches —
 * a match inside a matched slab adds nothing. One sort and one sweep, so a
 * search over a whole budget of slabs stays interactive.
 */
export function coveredSpan(slabs: [number, number][]): number {
  const sorted = slabs.toSorted((left, right) => left[0] - right[0]);
  let covered = 0;
  let reach = -Infinity;
  for (const [x0, x1] of sorted) {
    if (x1 > reach) { covered += x1 - Math.max(x0, reach); }
    reach = Math.max(reach, x1);
  }
  return covered;
}

/**
 * Clamp a flame-graph view window to config space: span forced into
 * [minSpan, 1], then the window slid back inside [0, 1] preserving the span.
 */
export function clampViewRange(
  x0: number,
  x1: number,
  minSpan: number,
): { x0: number; x1: number } {
  const span = Math.min(Math.max(x1 - x0, minSpan), 1);
  const left = Math.min(Math.max(x0, 0), 1 - span);
  return { x0: left, x1: left + span };
}
