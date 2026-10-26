// The flame layouts' slab budget, and the time-order layout it bounds.
// Implements [PROF-UI-SAMPLED]. A flame chart fuses adjacent identical stacks
// into slabs, but a deep program whose stack changes below the root on every
// sample (recursion of varying depth) still makes one slab per frame per
// sample: ten minutes of it is millions of slabs, more than a webview can be
// handed. A layout keeps at most FLAME_RECT_BUDGET slabs, the widest first and
// the shallowest among equals, and counts the rest. A slab is never wider than
// its parent nor shallower, so every slab kept keeps its parents.
import type { RenderRect, SpeedscopeProfile } from "./flame-model";

/** The most slabs one layout embeds: about 20 MB of JSON, well inside a webview. */
export const FLAME_RECT_BUDGET = 150_000;

/** A layout and how many of its narrowest slabs the budget left out. */
export interface BoundedRects {
  readonly rects: RenderRect[];
  readonly hidden: number;
}

/** Width buckets: bucket b holds slabs wider than total / 2^(b+1), up to total / 2^b. */
const BUCKETS = 64;

const bucketOf = (width: number, total: number): number =>
  Math.min(BUCKETS - 1, Math.floor(Math.log2(total / width)));

/** Slabs in bucket < `bucket`, and in `bucket` itself down to `depth`, are kept. */
interface Cutoff {
  readonly bucket: number;
  readonly depth: number;
}

const KEEP_ALL: Cutoff = { bucket: BUCKETS, depth: 0 };

function cutoffFor(counts: number[][], budget: number): Cutoff {
  let kept = 0;
  for (let bucket = 0; bucket < counts.length; bucket += 1) {
    const row = counts[bucket] ?? [];
    for (let depth = 0; depth < row.length; depth += 1) {
      kept += row[depth] ?? 0;
      if (kept > budget) return { bucket, depth: depth - 1 };
    }
  }
  return KEEP_ALL;
}

const keeps = (cutoff: Cutoff, bucket: number, depth: number): boolean =>
  bucket < cutoff.bucket || (bucket === cutoff.bucket && depth <= cutoff.depth);

/** Walk a layout's slabs twice: once to count them by width and depth, once to keep those within the budget. */
function bounded(walk: (emit: (rect: RenderRect) => void) => void, total: number, budget: number): BoundedRects {
  const counts: number[][] = [];
  walk((rect) => {
    const row = (counts[bucketOf(rect.totalWeight, total)] ??= []);
    row[rect.depth] = (row[rect.depth] ?? 0) + 1;
  });
  const cutoff = cutoffFor(counts, budget);
  const rects: RenderRect[] = [];
  let hidden = 0;
  walk((rect) => {
    if (keeps(cutoff, bucketOf(rect.totalWeight, total), rect.depth)) rects.push(rect);
    else hidden += 1;
  });
  return { rects, hidden };
}

/** `rects`, an already built layout over `total` weight, within the budget. */
export function boundedRects(rects: readonly RenderRect[], total: number, budget = FLAME_RECT_BUDGET): BoundedRects {
  return bounded((emit) => { rects.forEach(emit); }, total, budget);
}

// --- time-order layout ----------------------------------------------------------

interface OpenFrame {
  frameIdx: number;
  x0: number;
  self: number;
}

function commonPrefixLength(open: OpenFrame[], stack: number[]): number {
  let lcp = 0;
  while (lcp < open.length && lcp < stack.length && open[lcp].frameIdx === stack[lcp]) {
    lcp += 1;
  }
  return lcp;
}

function closeFramesAbove(open: OpenFrame[], keep: number, cursor: number, emit: (rect: RenderRect) => void): void {
  while (open.length > keep) {
    const frame = open.pop();
    if (frame !== undefined) {
      emit({ x0: frame.x0, x1: cursor, depth: open.length, frameIdx: frame.frameIdx,
        selfWeight: frame.self, totalWeight: cursor - frame.x0 });
    }
  }
}

/** Every time-order slab in weight units, closed in the order the samples end them. */
function walkTimeOrder(profile: SpeedscopeProfile, emit: (rect: RenderRect) => void): void {
  const open: OpenFrame[] = [];
  let cursor = 0;
  profile.samples.forEach((stack, i) => {
    closeFramesAbove(open, commonPrefixLength(open, stack), cursor, emit);
    for (let d = open.length; d < stack.length; d += 1) {
      open.push({ frameIdx: stack[d], x0: cursor, self: 0 });
    }
    if (open.length > 0) {
      open[open.length - 1].self += profile.weights[i];
    }
    cursor += profile.weights[i];
  });
  closeFramesAbove(open, 0, cursor, emit);
}

function normalizeRects(rects: RenderRect[], total: number): RenderRect[] {
  return rects
    .map((r) => ({ ...r, x0: r.x0 / total, x1: r.x1 / total }))
    .sort((a, b) => a.depth - b.depth || a.x0 - b.x0);
}

/** Flame-chart layout: x = time; adjacent identical stacks merge into slabs. */
export function timeOrderRects(profile: SpeedscopeProfile): RenderRect[] {
  return boundedTimeOrder(profile, Infinity).rects;
}

/** The time-order layout within the slab budget, as the panel embeds it. */
export function boundedTimeOrder(profile: SpeedscopeProfile, budget = FLAME_RECT_BUDGET): BoundedRects {
  const total = profile.weights.reduce((sum, w) => sum + w, 0);
  if (total <= 0) return { rects: [], hidden: 0 };
  const kept = bounded((emit) => { walkTimeOrder(profile, emit); }, total, budget);
  return { rects: normalizeRects(kept.rects, total), hidden: kept.hidden };
}
