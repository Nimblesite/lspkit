// Tests [PROF-UI-REFGRAPH]: the retention graph's layout is pure — many
// long-typed nodes in one layer still label every node, fitted with an
// ellipsis so no two labels overlap, and the shared scaler sizes a Retina
// canvas backing store. Plain mocha: the modules under test import no `vscode`.
import * as assert from "node:assert";
import { sizeCanvasToDevicePixels } from "../../canvas-hidpi";
import { fitLabelText } from "../../flame-script-helpers";
import type { RefGraphLabel, RefGraphLayout, RefLayoutNode } from "../../ref-graph-layout";
import {
  refBottomMargin,
  refCrowdedGap,
  refGraphLayout,
  refLabelPadding,
  refLayerHeight,
  refMinHeight,
  refTopMargin,
} from "../../ref-graph-layout";

const GRAPH_WIDTH = 900;
/** A holder type far wider than its layer's gap, as production filming showed (#4). */
const LONG_TYPE = "System.Collections.Concurrent.ConcurrentDictionary`2[String,Shop.Session]";
/** Monospace advance per character: 0.6 em. */
const MONOSPACE_ADVANCE = 0.6;
/** The type label's font size, which its char width (6px) follows from. */
const TYPE_FONT_HEIGHT = 10;
/** The layers a five-deep graph spans. */
const FIVE_DEEP = 5;

function holder(id: number): RefLayoutNode {
  return {
    id, depth: 1, type: LONG_TYPE, repr: `0x${String(id)}`, size: 4096,
    isTarget: false, isRoot: false, sizeLabel: "4.0 KB",
  };
}

/** The same holder, moved to another layer. */
function holderAt(id: number, depth: number): RefLayoutNode {
  return { ...holder(id), depth };
}

function everyLabel(layout: RefGraphLayout): RefGraphLabel[] {
  const labels: RefGraphLabel[] = [...layout.layerLabels];
  for (const node of layout.nodes.values()) {
    labels.push(...node.labels);
  }
  return labels;
}

function labelsOverlap(left: RefGraphLabel, right: RefGraphLabel): boolean {
  return left.x0 < right.x1 && right.x0 < left.x1 && left.y0 < right.y1 && right.y0 < left.y1;
}

function assertNeverOverlap(labels: readonly RefGraphLabel[]): void {
  for (let i = 0; i < labels.length; i += 1) {
    for (let j = i + 1; j < labels.length; j += 1) {
      assert.ok(!labelsOverlap(labels[i], labels[j]),
        `labels must not overlap: "${labels[i].text}" overlaps "${labels[j].text}"`);
    }
  }
}

suite("Retention graph layout stays pure and overlap-free", () => {
  test("many long-typed nodes in one layer never overlap a label", () => {
    const labels = everyLabel(refGraphLayout([holder(1), holder(2), holder(3), holder(4)], GRAPH_WIDTH, fitLabelText));
    assert.ok(labels.length >= 8, `four holders draw a type and a size label each, drew ${String(labels.length)}`);
    assertNeverOverlap(labels);
  });

  test("a long type is fitted to its gap with an ellipsis, never dropped", () => {
    const layout = refGraphLayout([holder(1), holder(2), holder(3), holder(4)], GRAPH_WIDTH, fitLabelText);
    const typeLabel = layout.nodes.get(1)?.labels.find((label) => label.kind === "type");
    const avail = GRAPH_WIDTH / 5 - refLabelPadding() * 2;
    assert.ok(typeLabel !== undefined, "a holder is labelled by its type");
    assert.ok(typeLabel.text.includes("…"), `a type wider than its gap is fitted, kept: ${typeLabel.text}`);
    assert.ok(typeLabel.text.length * MONOSPACE_ADVANCE * TYPE_FONT_HEIGHT <= avail,
      `the fitted type fits its slot of ${String(avail)}px`);
  });

  test("a layer tighter than the crowded gap names its type once, with a count", () => {
    const crowd = Array.from({ length: 8 }, (_unused, i): RefLayoutNode => holder(i + 1));
    assert.ok(GRAPH_WIDTH / (crowd.length + 1) < refCrowdedGap(), "the fixture really is crowded");
    const layout = refGraphLayout(crowd, GRAPH_WIDTH, fitLabelText);
    assert.ok([...layout.nodes.values()].every((node) => node.crowded && node.labels.length === 0),
      "a crowded layer draws no per-node labels");
    assert.strictEqual(layout.layerLabels.length, 1, "the layer is named once");
    assert.ok(layout.layerLabels[0].text.startsWith("8 × "),
      `the layer label counts its nodes: ${layout.layerLabels[0].text}`);
  });

  test("height follows the layers, floored at the minimum", () => {
    assert.strictEqual(refGraphLayout([holder(1)], GRAPH_WIDTH, fitLabelText).height, refMinHeight());
    const deep = Array.from({ length: FIVE_DEEP }, (_unused, i): RefLayoutNode => holderAt(i + 1, i));
    assert.strictEqual(
      refGraphLayout(deep, GRAPH_WIDTH, fitLabelText).height,
      FIVE_DEEP * refLayerHeight() + refBottomMargin(),
      "a five-deep graph spans five layers plus its label room",
    );
  });

  test("a node's row is its distance from the targets", () => {
    const deep = Array.from({ length: FIVE_DEEP }, (_unused, i): RefLayoutNode => holderAt(i + 1, i));
    const layout = refGraphLayout(deep, GRAPH_WIDTH, fitLabelText);
    const rowOf = (depth: number): number => refTopMargin() + (FIVE_DEEP - 1 - depth) * refLayerHeight();
    for (const [id, node] of layout.nodes) {
      assert.strictEqual(node.y, rowOf(id - 1), `node ${String(id)} sits in its layer`);
    }
  });

  test("the scaler sizes a Retina backing store and scales its context", () => {
    const canvas = { style: { width: "", height: "" }, width: 0, height: 0 };
    const transforms: number[][] = [];
    const ctx = {
      setTransform: (...matrix: readonly [number, number, number, number, number, number]): void => {
        transforms.push([...matrix]);
      },
    };
    sizeCanvasToDevicePixels({ canvas, ctx, cssWidth: 900, cssHeight: 400, devicePixelRatio: 2 });
    assert.strictEqual(canvas.width, 1800, "the backing width is 2× the CSS width");
    assert.strictEqual(canvas.height, 800, "the backing height is 2× the CSS height");
    assert.strictEqual(canvas.style.width, "900px", "the CSS width stays logical");
    assert.strictEqual(canvas.style.height, "400px", "the CSS height stays logical");
    assert.deepStrictEqual(transforms, [[2, 0, 0, 2, 0, 0]], "drawing stays in CSS pixels");
  });

  test("a non-positive ratio falls back to 1 and fractional ratios round", () => {
    const plain = { style: { width: "", height: "" }, width: 0, height: 0 };
    const transforms: number[][] = [];
    const ctx = {
      setTransform: (...matrix: readonly [number, number, number, number, number, number]): void => {
        transforms.push([...matrix]);
      },
    };
    sizeCanvasToDevicePixels({ canvas: plain, ctx, cssWidth: 900, cssHeight: 400, devicePixelRatio: 0 });
    assert.strictEqual(plain.width, 900, "a non-positive ratio is treated as 1");
    assert.deepStrictEqual(transforms, [[1, 0, 0, 1, 0, 0]], "the context is scaled by the fallback ratio");
    const fractional = { style: { width: "", height: "" }, width: 0, height: 0 };
    sizeCanvasToDevicePixels({ canvas: fractional, ctx, cssWidth: 901, cssHeight: 400, devicePixelRatio: 1.5 });
    assert.strictEqual(fractional.width, 1352, "the backing store rounds to whole device pixels");
  });
});
