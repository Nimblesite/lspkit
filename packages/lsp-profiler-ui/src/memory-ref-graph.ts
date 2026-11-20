// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Reference-graph webview for memory profiling.
 *
 * Renders the object-retention graph in a Canvas 2D webview, layered by each
 * object's distance from the targets: GC roots on top, named by their labels.
 * Extracted from `memory-profiler.ts` so that module stays focused on command
 * routing and the courier round-trip.
 *
 * Panel lifecycle, CSP, and safe data embedding come from the shared webview
 * host ([PROF-UI-WEBVIEW], profiler-webview.ts) — node `repr`/type strings
 * come from the profiled program and must never escape the inline script.
 */

import { cmd } from "./ids";
import { sizeCanvasToDevicePixels } from "./canvas-hidpi";
import { fitLabelText } from "./flame-script-helpers";
import {
  REF_MIN_WIDTH,
  belowLabels,
  collectLayer,
  crowdedLayerLabel,
  deepestDepth,
  fittedLabel,
  groupByDepth,
  layoutLayer,
  nodeLabels,
  nodeRadius,
  placeNode,
  refBottomMargin,
  refCrowdedGap,
  refGraphLayout,
  refLabelColor,
  refLabelFont,
  refLabelPadding,
  refLayerHeight,
  refMinHeight,
  refTopMargin,
  rootLabel,
  rowGeometry,
} from "./ref-graph-layout";
import {
  PROFILER_CSS_RESET,
  PROFILER_CSS_VARS,
  PROFILER_JS_UTILS,
  escapeHtml,
} from "./profiler-styles";
import {
  arrayField,
  booleanField,
  isRecord,
  numberField,
  recordArrayField,
  stringArrayField,
  stringField,
} from "./unknown-shape";
import {
  SingletonWebviewPanel,
  buildWebviewDocument,
  embedJson,
  handleSourceNavigation,
} from "./profiler-webview";

/** Reference-graph result returned by `<prefix>.lspProfiler.memory.ingest` (kind `refs`). */
export interface ReferenceGraphResult {
  targetType: string;
  maxDepth: number;
  maxNodes: number;
  script: string;
  graph?: {
    nodes: RefGraphNode[];
    edges: RefGraphEdge[];
    cycles: number[][];
    retentionPath?: string[];
  };
}

interface RefGraphNode {
  id: number;
  type: string;
  size: number;
  repr: string;
  depth: number;
  isTarget: boolean;
  /** A garbage-collection root; `repr` is then its root label, such as `static sessions`. */
  isRoot?: boolean;
  rootKind?: string | undefined;
}

interface RefGraphEdge {
  from: number;
  to: number;
  label: string;
}

/**
 * Decode the graph payload of a `kind: "refs"` ingest result.
 *
 * Node `repr`/`type` strings come straight from the profiled program, so every
 * field is coerced here rather than asserted — a malformed node becomes an
 * empty-ish node instead of an `undefined` the canvas renderer would trip on.
 */
export function decodeRefGraph(value: unknown): ReferenceGraphResult["graph"] {
  if (!isRecord(value)) {
    return undefined;
  }
  return {
    nodes: recordArrayField(value, "nodes").map((node) => toRefNode(node)),
    edges: recordArrayField(value, "edges").map((edge) => ({
      from: numberField(edge, "from") ?? 0,
      to: numberField(edge, "to") ?? 0,
      label: stringField(edge, "label") ?? "",
    })),
    cycles: arrayField(value, "cycles").map((cycle) =>
      Array.isArray(cycle) ? cycle.filter((id): id is number => typeof id === "number") : []
    ),
    retentionPath: stringArrayField(value, "retentionPath"),
  };
}

function toRefNode(node: Record<string, unknown>): RefGraphNode {
  return {
    id: numberField(node, "id") ?? 0,
    type: stringField(node, "type") ?? "",
    size: numberField(node, "size") ?? 0,
    repr: stringField(node, "repr") ?? "",
    depth: numberField(node, "depth") ?? 0,
    isTarget: booleanField(node, "isTarget") ?? false,
    isRoot: booleanField(node, "isRoot") ?? false,
    rootKind: stringField(node, "rootKind"),
  };
}

const refGraphPanel = new SingletonWebviewPanel(() => cmd("refGraph"), (msg) => {
  handleSourceNavigation(msg);
});

/** The open retention graph's view type, title and HTML (e2e seam). */
export function refGraphState(): { readonly viewType: string; readonly title: string; readonly html: string } | undefined {
  const described = refGraphPanel.describe();
  return described === undefined ? undefined : { ...described, html: refGraphPanel.html() ?? "" };
}

/** Open (or reveal) the retention-graph webview for a parsed reference graph. */
export function openRefGraphWebview(result: ReferenceGraphResult): void {
  refGraphPanel.show(`Retention Graph — ${result.targetType}`, buildRefGraphHtml(result));
}

/** Dispose the reference-graph webview, if open. */
export function disposeRefGraph(): void {
  refGraphPanel.dispose();
}

const REF_GRAPH_CSS = `${PROFILER_CSS_VARS}${PROFILER_CSS_RESET}
    body { padding: 16px; }
    h1 { font-size: 18px; font-weight: 600; margin-bottom: 12px; }
    h1 .accent { color: var(--prof-mem-critical); }
    .retention-path {
      background: var(--prof-surface);
      border: 1px solid var(--prof-border);
      border-radius: 8px;
      padding: 12px 16px;
      margin-bottom: 16px;
      font-family: 'JetBrains Mono', monospace;
      font-size: 12px;
      line-height: 1.8;
    }
    .retention-path .label {
      font-size: 11px;
      color: var(--prof-text-secondary);
      text-transform: uppercase;
      letter-spacing: 0.05em;
      margin-bottom: 6px;
    }
    .retention-path .step { color: var(--prof-info); }
    .retention-path .target { color: var(--prof-mem-critical); font-weight: 600; }
    canvas { display: block; width: 100%; border-radius: 8px; background: var(--prof-surface); }
    .legend {
      display: flex; gap: 16px; margin-top: 12px; font-size: 11px;
      color: var(--prof-text-secondary);
    }
    .legend-item { display: flex; align-items: center; gap: 4px; }
    .legend-dot { width: 8px; height: 8px; border-radius: 50%; }
    .no-data { text-align: center; padding: 60px; color: var(--prof-text-secondary); }`;

function buildRetentionPathHtml(retentionPath: string[]): string {
  if (retentionPath.length === 0) { return ""; }
  const steps = retentionPath
    .map((step, i) => `<div class="${i === retentionPath.length - 1 ? "target" : "step"}">${escapeHtml(step)}</div>`)
    .join("\n    ");
  return `<div class="retention-path">
    <div class="label">Retention Path</div>
    ${steps}
  </div>`;
}

/** The graph as script constants. */
function refGraphDataScript(nodesJson: string, edgesJson: string, cyclesJson: string): string {
  return `
    const nodes = ${nodesJson};
    const edges = ${edgesJson};
    const cycles = ${cyclesJson};
`;
}

/** The graph canvas: sized for its screen, then laid out by pure, tested code. */
const REF_GRAPH_LAYOUT_SCRIPT = `
    if (nodes.length === 0) {
      document.getElementById('graph').style.display = 'none';
      const noData = document.createElement('div');
      noData.className = 'no-data';
      noData.textContent = 'No reference graph data available. Run the memory references command with an active debug session.';
      document.body.appendChild(noData);
    } else {
      const canvas = document.getElementById('graph');
      const ctx = canvas.getContext('2d');
      // Fill the panel width so the graph is not letterboxed into 800px.
      const cssWidth = Math.max(${REF_MIN_WIDTH}, canvas.parentElement ? canvas.clientWidth : ${REF_MIN_WIDTH});
      const sized = nodes.map((n) => ({ ...n, sizeLabel: formatBytes(n.size) }));
      const layout = refGraphLayout(sized, cssWidth, fitLabelText);
      sizeCanvasToDevicePixels({ canvas: canvas, ctx: ctx, cssWidth: cssWidth, cssHeight: layout.height, devicePixelRatio: window.devicePixelRatio || 1 });
      const nodeMap = layout.nodes;
      const cycleNodeIds = new Set(cycles.flat());`;

/** Edges, a cycle's in the leak colour. */
const REF_GRAPH_EDGES_SCRIPT = `
      for (const edge of edges) {
        const a = nodeMap.get(edge.from), b = nodeMap.get(edge.to);
        if (!a || !b) continue;
        const isCycleEdge = cycleNodeIds.has(edge.from) && cycleNodeIds.has(edge.to);
        ctx.strokeStyle = isCycleEdge ? cssVar('--prof-mem-leak') : cssVar('--prof-border');
        ctx.lineWidth = isCycleEdge ? 2 : 1;
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        ctx.stroke();
        if (edge.label && !b.crowded) {
          ctx.fillStyle = cssVar('--prof-text-secondary');
          ctx.font = '9px monospace';
          ctx.textAlign = 'left';
          ctx.fillText(edge.label, (a.x + b.x) / 2 + 4, (a.y + b.y) / 2 - 4);
        }
      }`;

/** Nodes: targets, roots and cycles in the legend's colours; every label fitted by the layout. */
const REF_GRAPH_NODES_SCRIPT = `
      for (const n of nodeMap.values()) {
        const color = cssVar(n.isTarget ? '--prof-mem-critical' : n.isRoot ? '--prof-info' : cycleNodeIds.has(n.id) ? '--prof-mem-leak' : '--prof-text-secondary');
        ctx.beginPath();
        ctx.arc(n.x, n.y, n.radius, 0, Math.PI * 2);
        ctx.fillStyle = color;
        ctx.globalAlpha = 0.2;
        ctx.fill();
        ctx.globalAlpha = 1;
        ctx.strokeStyle = color;
        ctx.lineWidth = n.isTarget || n.isRoot ? 3 : 1.5;
        ctx.stroke();
        ctx.textAlign = 'center';
        for (const label of n.labels) {
          ctx.font = refLabelFont(label.kind);
          ctx.fillStyle = cssVar(refLabelColor(label.kind));
          ctx.fillText(label.text, label.x, label.y);
        }
      }`;

/** A crowded layer names its type once, with a count; the drawing branch closes. */
const REF_GRAPH_LAYER_LABELS_SCRIPT = `
      for (const label of layout.layerLabels) {
        ctx.font = refLabelFont(label.kind);
        ctx.fillStyle = cssVar(refLabelColor(label.kind));
        ctx.textAlign = 'center';
        ctx.fillText(label.text, label.x, label.y);
      }
    }`;

/** The legend, in the colours the canvas draws. */
const REF_GRAPH_LEGEND_HTML = `
  <div class="legend">
    <div class="legend-item"><div class="legend-dot" style="background: var(--prof-mem-critical)"></div> Target object</div>
    <div class="legend-item"><div class="legend-dot" style="background: var(--prof-info)"></div> GC root</div>
    <div class="legend-item"><div class="legend-dot" style="background: var(--prof-text-secondary)"></div> Intermediate</div>
    <div class="legend-item"><div class="legend-dot" style="background: var(--prof-mem-leak)"></div> Cycle member</div>
  </div>`;

/** Build the complete retention-graph HTML (exported as an e2e seam). */
export function buildRefGraphHtml(result: ReferenceGraphResult): string {
  const escapedType = escapeHtml(result.targetType);
  return buildWebviewDocument({
    title: `Retention Graph — ${escapedType}`,
    css: REF_GRAPH_CSS,
    body: `
  <h1><span class="accent">◉</span> Retention Graph — <span class="accent">${escapedType}</span></h1>
  ${buildRetentionPathHtml(result.graph?.retentionPath ?? [])}
  <canvas id="graph" width="800" height="500"></canvas>${REF_GRAPH_LEGEND_HTML}`,
    script: buildRefGraphScript(result),
  });
}

/**
 * The drawing script. Node types/reprs and edge labels are profiled-program
 * data, embedded so they can never close the inline <script> ([PROF-UI-WEBVIEW]).
 * Layout and device-pixel sizing are the compiled source of the pure,
 * unit-tested functions in ref-graph-layout.ts and canvas-hidpi.ts.
 */
function buildRefGraphScript(result: ReferenceGraphResult): string {
  const { graph } = result;
  return PROFILER_JS_UTILS +
    refGraphHelpersScript() +
    refGraphDataScript(embedJson(graph?.nodes ?? []), embedJson(graph?.edges ?? []), embedJson(graph?.cycles ?? [])) +
    REF_GRAPH_LAYOUT_SCRIPT + REF_GRAPH_EDGES_SCRIPT + REF_GRAPH_NODES_SCRIPT + REF_GRAPH_LAYER_LABELS_SCRIPT;
}

/** Every pure function the drawing script calls, as compiled source. */
function refGraphHelpersScript(): string {
  return [
    sizeCanvasToDevicePixels, fitLabelText, refBottomMargin, refCrowdedGap, refLabelPadding,
    refLayerHeight, refMinHeight, refTopMargin, groupByDepth, deepestDepth, collectLayer,
    rowGeometry, nodeRadius, placeNode, fittedLabel, refLabelFont, refLabelColor, belowLabels,
    rootLabel, nodeLabels, crowdedLayerLabel, layoutLayer, refGraphLayout,
  ].map((fn) => fn.toString()).join("\n");
}
