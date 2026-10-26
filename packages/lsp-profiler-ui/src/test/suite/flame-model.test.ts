// Tests for [PROF-UI-SAMPLED]: the results panel's flame model stays bounded
// however long a recording runs. A deep program whose stack differs below the
// root on every sample (recursion of varying depth) puts one time-order slab
// per frame per sample, which a ten-minute session turned into millions of
// slabs and a panel that failed with "Invalid string length".
import * as assert from "node:assert/strict";
import { buildFlameModel, parseSpeedscope, type RenderRect, type SpeedscopeFile } from "../../flame-model";
import { coveredSpan } from "../../flame-script-helpers";
import { FLAME_RECT_BUDGET } from "../../flame-time-order";

/** How many samples level `level` (1-based below the root) holds one frame for: shallow calls last, deep ones change every sample. */
const periodOf = (level: number): number => 2 ** Math.max(0, 12 - level);

/**
 * `samples` samples `depth` frames deep below a root, like recursion of
 * varying shape: each level alternates between two frames, every
 * `periodOf(level)` samples.
 */
function deepProfile(samples: number, depth: number): SpeedscopeFile {
  const stacks = Array.from({ length: samples }, (_, i) =>
    [0, ...Array.from({ length: depth }, (_, d) => 1 + (Math.floor(i / periodOf(d + 1)) % 2))]);
  return {
    shared: { frames: [{ name: "main" }, { name: "even" }, { name: "odd" }] },
    profiles: [{ type: "sampled", name: "main", unit: "seconds", startValue: 0, endValue: samples,
      samples: stacks, weights: stacks.map(() => 1) }],
  };
}

/** Every slab the profile holds: the root, and one per run at each level. */
function slabCount(samples: number, depth: number): number {
  return 1 + Array.from({ length: depth }, (_, d) => Math.ceil(samples / periodOf(d + 1))).reduce((sum, n) => sum + n, 0);
}

/** Whether a slab one level up encloses `child`: slabs at one depth never overlap, so a binary search on x0 finds it. */
function hasParent(level: readonly RenderRect[], child: RenderRect): boolean {
  let low = 0;
  let high = level.length - 1;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if ((level[middle]?.x0 ?? Infinity) <= child.x0) low = middle; else high = middle - 1;
  }
  const parent = level[low];
  return parent !== undefined && parent.x0 <= child.x0 && child.x1 <= parent.x1;
}

function levels(rects: readonly RenderRect[]): RenderRect[][] {
  const out: RenderRect[][] = [];
  for (const rect of rects) (out[rect.depth] ??= []).push(rect);
  return out.map((level) => level.sort((a, b) => a.x0 - b.x0));
}

suite("Flame model — bounded time order [PROF-UI-SAMPLED]", () => {
  test("a recording with more slabs than the budget keeps the widest, every kept slab's parents, and counts the rest", () => {
    const fiber = buildFlameModel(deepProfile(20_000, 40)).fibers[0];
    assert.ok(fiber !== undefined, "the profile has a fiber");
    assert.ok(fiber.timeOrder.length <= FLAME_RECT_BUDGET, `kept ${fiber.timeOrder.length} slabs`);
    assert.ok(fiber.timeOrderHidden > 0, "the narrow slabs dropped are counted, never silently");
    assert.ok(fiber.timeOrder.length > FLAME_RECT_BUDGET / 4, `the budget is used, not squandered: ${fiber.timeOrder.length}`);
    assert.equal(fiber.timeOrder.length + fiber.timeOrderHidden, slabCount(20_000, 40), "kept and hidden account for every slab");
    const narrowestKept = fiber.timeOrder.reduce((least, rect) => Math.min(least, rect.x1 - rect.x0), 1);
    assert.ok(fiber.timeOrder.filter((rect) => rect.depth <= 8).length === slabCount(20_000, 8), `every wide slab is kept (narrowest kept ${narrowestKept})`);
    const root = fiber.timeOrder.find((rect) => rect.depth === 0);
    assert.deepEqual([root?.x0, root?.x1], [0, 1], "the root still spans the whole recording");
    const byDepth = levels(fiber.timeOrder);
    const orphans = fiber.timeOrder.filter((rect) => rect.depth > 0 && !hasParent(byDepth[rect.depth - 1] ?? [], rect));
    assert.deepEqual(orphans.slice(0, 3), [], "every kept slab keeps its parent");
    assert.ok(JSON.stringify(buildFlameModel(deepProfile(20_000, 40))).length < 64 * 1024 * 1024, "the model embeds in a webview");
  });

  test("a recording within the budget keeps every slab and hides none", () => {
    const fiber = buildFlameModel(deepProfile(100, 5)).fibers[0];
    assert.ok(fiber !== undefined, "the profile has a fiber");
    assert.equal(fiber.timeOrder.length, slabCount(100, 5), "every slab is kept");
    assert.equal(fiber.timeOrderHidden, 0);
    assert.equal(fiber.leftHeavyHidden, 0, "the aggregated view is small and kept whole");
  });
});

/** A one-profile speedscope document whose profile is `profile`, as JSON text. */
const documentWith = (profile: unknown): string =>
  JSON.stringify({ shared: { frames: [{ name: "main" }] }, profiles: [profile] });

const PROFILE = { type: "sampled", name: "main", unit: "seconds", startValue: 0, endValue: 1, samples: [[0]] };

suite("Flame model — a malformed export is a classified error [PROF-UI-SAMPLED]", () => {
  test("a profile that is not an object is refused, never thrown", () => {
    for (const profile of [null, 7, "sampled"]) {
      const parsed = parseSpeedscope(documentWith(profile));
      assert.equal(parsed.ok, false, `${JSON.stringify(profile)} is refused`);
    }
  });

  test("a weight that is not a finite, non-negative number is refused", () => {
    for (const weight of [null, -1, "2", {}]) {
      const parsed = parseSpeedscope(documentWith({ ...PROFILE, weights: [weight] }));
      assert.deepEqual(parsed, { ok: false, error: 'profile "main" has a weight that is not a non-negative number' });
    }
  });

  test("a well-formed profile still parses", () => {
    assert.equal(parseSpeedscope(documentWith({ ...PROFILE, weights: [0.5] })).ok, true);
  });
});

suite("Flame search — the matched share counts nested matches once [PROF-UI-SAMPLED]", () => {
  test("a match inside a matched slab adds nothing; disjoint matches add up", () => {
    assert.equal(coveredSpan([[0, 0.5], [0.125, 0.25], [0.625, 0.875], [0.75, 0.8125]]), 0.75);
  });

  test("the share is the same whatever order the slabs arrive in", () => {
    assert.equal(coveredSpan([[0.75, 0.8125], [0.625, 0.875], [0.125, 0.25], [0, 0.5]]), 0.75);
    assert.equal(coveredSpan([]), 0);
  });
});
