// Implements [PROF-UI-REFGRAPH]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Pure layout for the retention-graph canvas: every node's position, radius
 * and the exact labels drawn for it, each fitted to its slot so no two labels
 * ever overlap — a layer of many long-typed nodes still names every node (#4).
 *
 * buildRefGraphScript embeds these functions' COMPILED SOURCE via
 * Function.prototype.toString, so the webview executes the same type-checked
 * code these unit tests run. The rules of flame-script-helpers apply, plus two
 * the CommonJS output forces: no module-level constants (an exported const
 * compiles to `exports.X`, which does not exist in the webview — metrics are
 * exported as functions instead), and no imports (a call compiles to a
 * namespaced require — the label fitter is passed in explicitly).
 */

/** Fits a label into `avail` px; the webview passes the embedded fitLabelText. */
export type LabelFitter = (text: string, avail: number, measure: (text: string) => number) => string;

/** Vertical distance between layers, in CSS pixels. */
export function refLayerHeight(): number {
  const LAYER_HEIGHT = 120;
  return LAYER_HEIGHT;
}

/** Margin above the top layer. */
export function refTopMargin(): number {
  const TOP_MARGIN = 60;
  return TOP_MARGIN;
}

/** Room below the deepest layer for its labels. */
export function refBottomMargin(): number {
  const BOTTOM_MARGIN = 40;
  return BOTTOM_MARGIN;
}

/** The canvas is never drawn shorter than this, whatever the panel. */
export function refMinHeight(): number {
  const MIN_HEIGHT = 360;
  return MIN_HEIGHT;
}

/** Below this gap a layer is too tight to label each node ([PROF-UI-REFGRAPH]). */
export function refCrowdedGap(): number {
  const CROWDED_GAP = 160;
  return CROWDED_GAP;
}

/** Clearance each side of a label, so neighbours never touch. */
export function refLabelPadding(): number {
  const LABEL_PADDING = 8;
  return LABEL_PADDING;
}

/** The canvas is never drawn narrower than this, whatever the panel (TS-side only). */
export const REF_MIN_WIDTH = 800;

/** A node as the layout consumes it: graph fields plus its formatted size. */
export interface RefLayoutNode {
  readonly id: number;
  readonly depth: number;
  readonly type: string;
  readonly repr: string;
  readonly size: number;
  readonly isTarget: boolean;
  readonly isRoot: boolean;
  /** The size formatted for display, for example "4.0 KB". */
  readonly sizeLabel: string;
}

/** What a label names: its root's label, the type, the size, or a crowded layer. */
export type RefLabelKind = "root" | "type" | "size" | "layer";

/** One drawn label: its anchor and baseline, and the box it occupies. */
export interface RefGraphLabel {
  readonly kind: RefLabelKind;
  readonly text: string;
  readonly x: number;
  readonly y: number;
  readonly x0: number;
  readonly y0: number;
  readonly x1: number;
  readonly y1: number;
}

/** One placed node: its id, centre, radius and the labels drawn for it. */
export interface RefGraphNodeLayout {
  readonly id: number;
  readonly x: number;
  readonly y: number;
  readonly radius: number;
  readonly crowded: boolean;
  readonly labels: readonly RefGraphLabel[];
}

/** A whole graph's layout: its nodes by id, its crowded layers' labels, its height. */
export interface RefGraphLayout {
  readonly nodes: ReadonlyMap<number, RefGraphNodeLayout>;
  readonly layerLabels: readonly RefGraphLabel[];
  readonly height: number;
}

/** One layer's input: its members and the graph-wide geometry they share. */
export interface LayerPlan {
  readonly members: readonly RefLayoutNode[];
  readonly depth: number;
  readonly top: number;
  readonly width: number;
  readonly fit: LabelFitter;
}

/** One layer's placed nodes, and its label when the layer is crowded. */
export interface LayerOut {
  readonly nodes: readonly RefGraphNodeLayout[];
  readonly label: RefGraphLabel | undefined;
}

/** Shared geometry of one layer's row: its gap, band and label room. */
export interface RowGeometry {
  readonly gap: number;
  readonly y: number;
  readonly avail: number;
  readonly crowded: boolean;
}

/** One node's slot: where its centre sits and the room its labels have. */
export interface NodeSlot {
  readonly x: number;
  readonly y: number;
  readonly gap: number;
  readonly avail: number;
  readonly crowded: boolean;
}

/** Where one node sits: its slot geometry plus its own radius. */
export interface NodePlace {
  readonly x: number;
  readonly y: number;
  readonly radius: number;
  readonly avail: number;
  readonly crowded: boolean;
}

/** What one label draws: its kind, text, anchor, metrics and room. */
export interface LabelSpec {
  readonly kind: RefLabelKind;
  readonly text: string;
  readonly x: number;
  readonly baseline: number;
  readonly charWidth: number;
  readonly fontHeight: number;
  readonly avail: number;
}

/** Lay out a whole graph: every node's place, radius and fitted labels. */
export function refGraphLayout(nodes: readonly RefLayoutNode[], width: number, fit: LabelFitter): RefGraphLayout {
  const top = deepestDepth(nodes);
  const height = Math.max(refMinHeight(), (top + 1) * refLayerHeight() + refBottomMargin());
  const layouts = new Map<number, RefGraphNodeLayout>();
  const layerLabels: RefGraphLabel[] = [];
  for (const [depth, members] of groupByDepth(nodes)) {
    collectLayer(layoutLayer({ members, depth, top, width, fit }), layouts, layerLabels);
  }
  return { nodes: layouts, layerLabels, height };
}

/** The deepest layer: roots' depth, which anchors every row. */
export function deepestDepth(nodes: readonly RefLayoutNode[]): number {
  let deepest = 0;
  for (const node of nodes) { deepest = Math.max(deepest, node.depth); }
  return deepest;
}

/** Fold one laid-out layer into the graph's nodes and layer labels. */
export function collectLayer(
  layer: LayerOut,
  layouts: Map<number, RefGraphNodeLayout>,
  layerLabels: RefGraphLabel[],
): void {
  for (const node of layer.nodes) { layouts.set(node.id, node); }
  if (layer.label !== undefined) { layerLabels.push(layer.label); }
}

/** Nodes grouped by their layer: distance from the targets. */
export function groupByDepth(nodes: readonly RefLayoutNode[]): Map<number, readonly RefLayoutNode[]> {
  const layers = new Map<number, RefLayoutNode[]>();
  for (const node of nodes) {
    const members = layers.get(node.depth) ?? [];
    members.push(node);
    layers.set(node.depth, members);
  }
  return layers;
}

/** Place one layer's nodes across the width, or name them once when crowded. */
export function layoutLayer(plan: LayerPlan): LayerOut {
  const row = rowGeometry(plan);
  const nodes = plan.members.map((node, index) =>
    placeNode(node, { x: row.gap * (index + 1), y: row.y, gap: row.gap, avail: row.avail, crowded: row.crowded }, plan.fit));
  const label = row.crowded ? crowdedLayerLabel(plan, row, nodes[0].radius) : undefined;
  return { nodes, label };
}

/** The row one layer occupies: its gap, its band, and its labels' room. */
export function rowGeometry(plan: LayerPlan): RowGeometry {
  const gap = plan.width / (plan.members.length + 1);
  return {
    gap,
    crowded: gap < refCrowdedGap(),
    y: refTopMargin() + (plan.top - plan.depth) * refLayerHeight(),
    avail: Math.max(gap - refLabelPadding() * 2, 1),
  };
}

/** Place one node at its slot: its radius, and its fitted labels. */
export function placeNode(node: RefLayoutNode, slot: NodeSlot, fit: LabelFitter): RefGraphNodeLayout {
  const radius = nodeRadius(node.size, slot.gap, slot.crowded);
  const place = { x: slot.x, y: slot.y, radius, avail: slot.avail, crowded: slot.crowded };
  const labels = nodeLabels(node, place, fit);
  return { id: node.id, x: slot.x, y: slot.y, radius, crowded: slot.crowded, labels };
}

/** A node's radius: logarithmic in its size, capped by its layer's gap. */
export function nodeRadius(size: number, gap: number, crowded: boolean): number {
  const MIN_RADIUS = 8;
  const CROWDED_SHARE = 2.5;
  const MAX_RADIUS = 30;
  const bySize = Math.max(MIN_RADIUS, Math.log2(Math.max(size, 1)) * 2);
  return Math.min(crowded ? gap / CROWDED_SHARE : MAX_RADIUS, bySize);
}

/** The type and size labels below a node, each in its own font metrics. */
export function belowLabels(node: RefLayoutNode, place: NodePlace, fit: LabelFitter): RefGraphLabel[] {
  // Monospace advance is 0.6 em, so each char width follows its font size.
  const TYPE_CHAR = 6;
  const TYPE_FONT = 10;
  const TYPE_BASE = 14;
  const SIZE_CHAR = 5.4;
  const SIZE_FONT = 9;
  const SIZE_BASE = 26;
  const below = place.y + place.radius;
  const typeSpec: LabelSpec = { kind: "type", text: node.type, x: place.x, baseline: below + TYPE_BASE, charWidth: TYPE_CHAR, fontHeight: TYPE_FONT, avail: place.avail };
  const sizeSpec: LabelSpec = { kind: "size", text: node.sizeLabel, x: place.x, baseline: below + SIZE_BASE, charWidth: SIZE_CHAR, fontHeight: SIZE_FONT, avail: place.avail };
  return [fittedLabel(typeSpec, fit), fittedLabel(sizeSpec, fit)];
}

/** A root's label above its node, in the root's bold metrics. */
export function rootLabel(node: RefLayoutNode, place: NodePlace, fit: LabelFitter): RefGraphLabel {
  const ROOT_CHAR = 7.8;
  const ROOT_FONT = 13;
  const ROOT_BASE = 10;
  const baseline = place.y - place.radius - ROOT_BASE;
  const spec: LabelSpec = { kind: "root", text: node.repr, x: place.x, baseline, charWidth: ROOT_CHAR, fontHeight: ROOT_FONT, avail: place.avail };
  return fittedLabel(spec, fit);
}

/** The labels one node draws: its type and size, and a root's label above. */
export function nodeLabels(node: RefLayoutNode, place: NodePlace, fit: LabelFitter): RefGraphLabel[] {
  if (place.crowded) { return []; }
  const labels = belowLabels(node, place, fit);
  return node.isRoot ? [rootLabel(node, place, fit), ...labels] : labels;
}

/** A crowded layer's single label: its count, and its type when shared. */
export function crowdedLayerLabel(plan: LayerPlan, row: RowGeometry, radius: number): RefGraphLabel {
  const LAYER_CHAR = 6.6;
  const LAYER_FONT = 11;
  const LAYER_BASE = 18;
  const types = new Set(plan.members.map((member) => member.type));
  const shared = types.size === 1 ? plan.members[0].type : "objects";
  const text = `${plan.members.length} × ${shared}`;
  const avail = plan.width - refLabelPadding() * 2;
  return fittedLabel({ kind: "layer", text, x: plan.width / 2, baseline: row.y + radius + LAYER_BASE, charWidth: LAYER_CHAR, fontHeight: LAYER_FONT, avail }, plan.fit);
}

/**
 * One label, fitted to `avail` px with an ellipsis so it never reaches its
 * neighbours, with the box it occupies: centred on its anchor, its ascent
 * above the baseline and a quarter em below.
 */
export function fittedLabel(spec: LabelSpec, fit: LabelFitter): RefGraphLabel {
  const DESCENDER = 4;
  const fitted = fit(spec.text, spec.avail, (candidate: string): number => candidate.length * spec.charWidth);
  const half = (fitted.length * spec.charWidth) / 2;
  return {
    kind: spec.kind, text: fitted, x: spec.x, y: spec.baseline,
    x0: spec.x - half, x1: spec.x + half,
    y0: spec.baseline - spec.fontHeight, y1: spec.baseline + spec.fontHeight / DESCENDER,
  };
}

/** The canvas font each label kind draws in — the layout's metric source. */
export function refLabelFont(kind: RefLabelKind): string {
  if (kind === "root") { return "bold 13px monospace"; }
  if (kind === "size") { return "9px monospace"; }
  if (kind === "layer") { return "11px monospace"; }
  return "10px monospace";
}

/** The CSS variable each label kind draws in. */
export function refLabelColor(kind: RefLabelKind): string {
  if (kind === "root") { return "--prof-info"; }
  if (kind === "size") { return "--prof-text-secondary"; }
  return "--prof-text";
}
