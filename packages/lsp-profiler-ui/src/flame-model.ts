// Shared sampled flame-graph model. Implements [PROF-UI-SAMPLED].
// Parses the speedscope "sampled" export produced by a runtime
// and precomputes everything the canvas renderer needs: a LEFT-HEAVY layout
// (samples merged into an aggregated tree, heaviest children first) and a
// TIME-ORDER layout (a flame chart where adjacent identical stacks fuse into
// continuous slabs), plus per-frame self/total weights, deterministic frame
// colors, and substring search. No vscode imports — everything here is unit
// testable and JSON-serializable for embedding in the webview.

import { boundedRects, boundedTimeOrder } from "./flame-time-order";
import { isRecord } from "./unknown-shape";
import { frameRanks, hasSourceFile } from "./palette";

export { timeOrderRects } from "./flame-time-order";
export { byFrameKeyThenIndex, type FrameRank } from "./palette";
export { searchMatches } from "./flame-script-helpers";

export interface SpeedscopeFrame {
  name: string;
  file?: string;
  line?: number;
}

export interface SpeedscopeProfile {
  type: string;
  name: string;
  unit: string;
  startValue: number;
  endValue: number;
  /** Stacks as ROOT-FIRST frame-index arrays, one per sample. */
  samples: number[][];
  /** Weight of each sample, in `unit` (seconds for Osprey exports). */
  weights: number[];
}

export interface SpeedscopeFile {
  shared: { frames: SpeedscopeFrame[] };
  profiles: SpeedscopeProfile[];
}

export type Result<Value> = { ok: true; value: Value } | { ok: false; error: string };

/** One renderable slab. x0/x1 are in [0,1] config space; weights in seconds. */
export interface RenderRect {
  x0: number;
  x1: number;
  depth: number;
  frameIdx: number;
  selfWeight: number;
  totalWeight: number;
}

/** Per-frame aggregates for one fiber, indexed by frameIdx. */
export interface FrameStats {
  self: number[];
  total: number[];
  count: number[];
}

export interface FiberFlame {
  name: string;
  totalWeight: number;
  sampleCount: number;
  maxDepthLeft: number;
  maxDepthTime: number;
  leftHeavy: RenderRect[];
  timeOrder: RenderRect[];
  /** Narrow slabs each layout's budget left undrawn (see flame-time-order.ts). */
  leftHeavyHidden: number;
  timeOrderHidden: number;
  stats: FrameStats;
}

export interface FrameMeta {
  source?: boolean;
  name: string;
  file: string;
  line: number;
  color: string;
}

export interface FlameModel {
  frames: FrameMeta[];
  fibers: FiberFlame[];
}

// --- parsing -----------------------------------------------------------------

function isFrame(value: unknown): value is SpeedscopeFrame {
  return isRecord(value) && typeof value.name === "string";
}

const isWeight = (weight: unknown): boolean =>
  typeof weight === "number" && Number.isFinite(weight) && weight >= 0;

const isStack = (stack: unknown, frameCount: number): boolean =>
  Array.isArray(stack) && stack.every((frame) => Number.isInteger(frame) && frame >= 0 && frame < frameCount);

/** What is wrong with one profile, if anything. */
function profileError(raw: unknown, frameCount: number): string | undefined {
  if (!isRecord(raw)) { return "speedscope file has a profile that is not an object"; }
  const name = String(raw.name);
  return profileShapeError(raw, name, frameCount);
}

/** What is wrong with the shape of one parsed profile. */
function profileShapeError(raw: Record<string, unknown>, name: string, frameCount: number): string | undefined {
  const { type, samples, weights } = raw;
  if (type !== "sampled") { return `profile "${name}" has unsupported type "${String(type)}"`; }
  if (!Array.isArray(samples) || !Array.isArray(weights)) { return `profile "${name}" is missing samples/weights`; }
  if (samples.length !== weights.length) { return `profile "${name}" has ${String(samples.length)} samples but ${String(weights.length)} weights`; }
  if (!weights.every((weight) => isWeight(weight))) { return `profile "${name}" has a weight that is not a non-negative number`; }
  return samples.every((stack) => isStack(stack, frameCount)) ? undefined : `profile "${name}" references a frame outside shared.frames`;
}

/** What is wrong with a parsed speedscope document, if anything. */
function speedscopeError(doc: unknown): string | undefined {
  const shared = isRecord(doc) ? doc.shared : undefined;
  const frames = isRecord(shared) ? shared.frames : undefined;
  if (!Array.isArray(frames) || !frames.every((frame) => isFrame(frame))) { return "speedscope file is missing shared.frames"; }
  const profiles = isRecord(doc) ? doc.profiles : undefined;
  if (!Array.isArray(profiles) || profiles.length === 0) { return "speedscope file has no profiles"; }
  return profiles.map((profile) => profileError(profile, frames.length)).find((error) => error !== undefined);
}

/** The document's outline; `speedscopeError` has checked everything inside it. */
function isSpeedscopeFile(doc: unknown): doc is SpeedscopeFile {
  return isRecord(doc) && isRecord(doc.shared) && Array.isArray(doc.profiles);
}

/** JSON.parse into a Result — shared by both profiler artifact parsers. */
export function parseJsonResult(text: string, what: string): Result<unknown> {
  try {
    return { ok: true, value: JSON.parse(text) };
  } catch (error) {
    return { ok: false, error: `${what} is not valid JSON: ${String(error)}` };
  }
}

/** Parse + validate a speedscope JSON document. Never throws. */
export function parseSpeedscope(text: string): Result<SpeedscopeFile> {
  const raw = parseJsonResult(text, "speedscope file");
  if (!raw.ok) {
    return raw;
  }
  const error = speedscopeError(raw.value);
  return error === undefined && isSpeedscopeFile(raw.value)
    ? { ok: true, value: raw.value }
    : { ok: false, error: error ?? "speedscope file is missing shared.frames" };
}

// --- left-heavy layout ---------------------------------------------------------

interface AggNode {
  frameIdx: number;
  total: number;
  self: number;
  children: Map<number, AggNode>;
}

function childOf(children: Map<number, AggNode>, frameIdx: number): AggNode {
  const existing = children.get(frameIdx);
  if (existing !== undefined) {
    return existing;
  }
  const node: AggNode = { frameIdx, total: 0, self: 0, children: new Map() };
  children.set(frameIdx, node);
  return node;
}

function insertStack(
  root: Map<number, AggNode>,
  stack: number[],
  weight: number,
): void {
  let children = root;
  let node: AggNode | undefined = undefined;
  for (const frameIdx of stack) {
    node = childOf(children, frameIdx);
    node.total += weight;
    ({ children } = node);
  }
  if (node !== undefined) {
    node.self += weight;
  }
}

function sortedByWeight(children: Map<number, AggNode>): AggNode[] {
  return [...children.values()].toSorted(
    (left, right) => right.total - left.total || left.frameIdx - right.frameIdx,
  );
}

interface AggEmitState {
  x: number;
  depth: number;
  scale: number;
  out: RenderRect[];
}

function emitAggNodes(nodes: AggNode[], state: AggEmitState): void {
  for (const node of nodes) {
    state.out.push({
      x0: state.x * state.scale,
      x1: (state.x + node.total) * state.scale,
      depth: state.depth,
      frameIdx: node.frameIdx,
      selfWeight: node.self,
      totalWeight: node.total,
    });
    emitAggNodes(sortedByWeight(node.children), {
      x: state.x,
      depth: state.depth + 1,
      scale: state.scale,
      out: state.out,
    });
    state.x += node.total;
  }
}

/** Merge samples into the aggregated left-heavy layout (heaviest first). */
export function leftHeavyRects(profile: SpeedscopeProfile): RenderRect[] {
  const total = profile.weights.reduce((sum, weight) => sum + weight, 0);
  if (total <= 0) {
    return [];
  }
  const root = new Map<number, AggNode>();
  for (const [index, stack] of profile.samples.entries()) {
    insertStack(root, stack, profile.weights[index]);
  }
  const out: RenderRect[] = [];
  emitAggNodes(sortedByWeight(root), { x: 0, depth: 0, scale: 1 / total, out });
  return out;
}

// --- per-frame aggregates ---------------------------------------------------------

/** Self/total weight and sample count per frameIdx (recursion deduplicated). */
function zeros(count: number): number[] {
  return Array.from({ length: count }, () => 0);
}

export function frameStats(
  profile: SpeedscopeProfile,
  frameCount: number,
): FrameStats {
  const stats: FrameStats = { self: zeros(frameCount), total: zeros(frameCount), count: zeros(frameCount) };
  for (const [index, stack] of profile.samples.entries()) {
    const weight = profile.weights[index];
    for (const frameIdx of new Set(stack)) {
      stats.total[frameIdx] += weight;
      stats.count[frameIdx] += 1;
    }
    const leaf = stack.at(-1);
    if (leaf !== undefined) {
      stats.self[leaf] += weight;
    }
  }
  return stats;
}

// --- colors -----------------------------------------------------------------------

const COLOR_CHANNEL_MAX = 255;
const TRIANGLE_HALF = 0.5;
const RUNTIME_HUE = 215;
const RUNTIME_HUE_MIX_STEPS = 30;
const RUNTIME_LIGHTNESS_BASE = 58;
const RUNTIME_LIGHTNESS_RANGE = 12;
const SOURCE_HUE_RANGE = 324;
const SOURCE_SATURATION_BASE = 25;
const SOURCE_SATURATION_RANGE = 20;
const SOURCE_LIGHTNESS_BASE = 65;
const SOURCE_LIGHTNESS_RANGE = 15;

/** Triangle wave in [0,1] with period 2 — speedscope's color mixer. */
export const triangle = (value: number): number =>
  2 * Math.abs(value / 2 - Math.floor(value / 2 + TRIANGLE_HALF));

/** One frame's place in the color ramp: its file and its rank among `frameCount` frames. */
export interface RankedFrame {
  readonly file: string;
  readonly rank: number;
  readonly frameCount: number;
}

/** Deterministic speedscope-style color for the frame ranked `rank` of `frameCount`. */
export function colorForRank({ file, rank, frameCount }: RankedFrame, isSource = hasSourceFile): string {
  const bucket = Math.floor((COLOR_CHANNEL_MAX * rank) / Math.max(frameCount, 1));
  const hue = bucket / COLOR_CHANNEL_MAX;
  const mix = triangle(RUNTIME_HUE_MIX_STEPS * hue);
  if (!isSource(file)) {
    return `hsl(${String(RUNTIME_HUE)}, 12%, ${(RUNTIME_LIGHTNESS_BASE - RUNTIME_LIGHTNESS_RANGE * mix).toFixed(1)}%)`;
  }
  return `hsl(${(SOURCE_HUE_RANGE * hue).toFixed(1)}, ${(SOURCE_SATURATION_BASE + SOURCE_SATURATION_RANGE * mix).toFixed(1)}%, ${(SOURCE_LIGHTNESS_BASE - SOURCE_LIGHTNESS_RANGE * mix).toFixed(1)}%)`;
}

/** Stable per-frame colors: frames ranked by (file+name), ramped over hue. */
export function frameColors(frames: SpeedscopeFrame[], isSource = hasSourceFile): string[] {
  const ranks = frameRanks(frames);
  return frames.map((frame, i) => colorForRank({ file: frame.file ?? "", rank: ranks[i] ?? i, frameCount: frames.length }, isSource));
}

// --- assembly ------------------------------------------------------------------------

function maxDepth(rects: RenderRect[]): number {
  let deepest = 0;
  for (const rect of rects) {
    deepest = Math.max(deepest, rect.depth + 1);
  }
  return deepest;
}

function buildFiber(profile: SpeedscopeProfile, frameCount: number): FiberFlame {
  const totalWeight = profile.weights.reduce((sum, weight) => sum + weight, 0);
  const leftHeavy = boundedRects(leftHeavyRects(profile), totalWeight);
  const timeOrder = boundedTimeOrder(profile);
  return {
    name: profile.name,
    totalWeight,
    sampleCount: profile.samples.length,
    maxDepthLeft: maxDepth(leftHeavy.rects),
    maxDepthTime: maxDepth(timeOrder.rects),
    leftHeavy: leftHeavy.rects,
    timeOrder: timeOrder.rects,
    leftHeavyHidden: leftHeavy.hidden,
    timeOrderHidden: timeOrder.hidden,
    stats: frameStats(profile, frameCount),
  };
}

/** The complete serializable model for one speedscope document. */
export function buildFlameModel(doc: SpeedscopeFile, isSource = hasSourceFile): FlameModel {
  const colors = frameColors(doc.shared.frames, isSource);
  return {
    frames: doc.shared.frames.map((frame, i) => ({
      name: frame.name,
      file: frame.file ?? "",
      line: frame.line ?? 0,
      color: colors[i],
      source: isSource(frame.file ?? ""),
    })),
    fibers: doc.profiles.map((profile) => buildFiber(profile, doc.shared.frames.length)),
  };
}
