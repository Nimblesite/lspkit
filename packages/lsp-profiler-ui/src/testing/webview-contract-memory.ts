// Implements [PROF-UI-WEBVIEW] + [PROF-UI-REFGRAPH]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The memory dashboard and the retention graph are CSP-locked, keep
 * profiled-program data inert, and say plainly what they could not show.
 */

import * as assert from 'node:assert';
import * as vm from 'node:vm';
import type { MemoryDiffData } from '../memory-dashboard';
import { buildMemoryDashboardHtml } from '../memory-dashboard';
import type { ReferenceGraphResult } from '../memory-ref-graph';
import { buildRefGraphHtml, decodeRefGraph } from '../memory-ref-graph';
import { buildFlamegraphHtml } from '../profiler-flamegraph-html';
import { embedJson } from '../profiler-webview';
import { stringField } from '../unknown-shape';
import { HOSTILE, dashboardSnapshot, profileResult } from './webview-contract-helpers';

interface DrawnText {
    readonly text: string;
    readonly y: number;
    readonly fill: string;
}

/** A no-op for canvas methods the recording mock does not implement. */
function noopCanvasMethod(): void {
    // Recording mocks only implement the methods the graph script draws with.
}

/** A getComputedStyle stand-in: every CSS variable resolves to its own name. */
const mockGetComputedStyle = (): { getPropertyValue: (name: string) => string } => ({
    getPropertyValue: (name: string): string => name,
});

/** The recording canvas context's pen, typed for the proxy reads below. */
interface RecordingPen {
    fillStyle: string;
    fillText: (text: string, x: number, y: number) => void;
    readonly [key: string]: unknown;
}

/** The minimal canvas surface the inline graph script reads and draws on. */
interface RecordingCanvas {
    readonly width: number;
    readonly height: number;
    readonly clientWidth: number;
    readonly parentElement: Record<string, never>;
    readonly style: Record<string, never>;
    readonly getContext: () => RecordingPen;
}

/** The minimal document surface the inline graph script queries. */
interface RecordingDocument {
    readonly getElementById: (id: string) => RecordingCanvas;
    readonly documentElement: Record<string, never>;
    readonly createElement: (tag: string) => { style: Record<string, never> };
    readonly body: { appendChild: (child: unknown) => void };
}

/**
 * Run a retention graph's inline script against a recording canvas and return
 * every text it draws, with where and in which colour: the panel as a reader
 * sees it. Each CSS variable resolves to its own name, so two different
 * variables are two different colours.
 */
function drawnTexts(html: string): DrawnText[] {
    const script = /<script nonce="[^"]+">(?<body>[\s\S]*)<\/script>/u.exec(html)?.groups?.body ?? '';
    const texts: DrawnText[] = [];
    const pen: RecordingPen = {
        fillStyle: '',
        fillText: (text: string, _x: number, y: number): void => { texts.push({ text, y, fill: pen.fillStyle }); },
    };
    const ctx = recordingCtx(pen);
    const canvas = recordingCanvas(ctx);
    const document = recordingDocument(canvas);
    vm.runInNewContext(script, { document, getComputedStyle: mockGetComputedStyle, window: { addEventListener: noopCanvasMethod } });
    return texts;
}

function recordingCtx(pen: RecordingPen): RecordingPen {
    return new Proxy(pen, {
        get: (target: RecordingPen, key: string | symbol): unknown => {
            if (typeof key !== 'string') { return noopCanvasMethod; }
            return key in target ? target[key] : noopCanvasMethod;
        },
    });
}

function recordingCanvas(ctx: RecordingPen): RecordingCanvas {
    return {
        width: 800, height: 500, clientWidth: 900, parentElement: {}, style: {},
        getContext: (): RecordingPen => ctx,
    };
}

function recordingDocument(canvas: RecordingCanvas): RecordingDocument {
    return {
        getElementById: (): RecordingCanvas => canvas,
        documentElement: {},
        createElement: (): { style: Record<string, never> } => ({ style: {} }),
        body: { appendChild: noopCanvasMethod },
    };
}

function hostileDashboardIsInert(): void {
    const snapshot = dashboardSnapshot({ topAllocations: [{ file: HOSTILE, line: 1, size: 1024, count: 2 }] });
    const diff: MemoryDiffData = {
        totalGrowth: 2048, totalFreed: 0, netGrowth: 2048, grownAllocations: [],
        suspectedLeaks: [{
            file: HOSTILE, line: 1, sizeGrowth: 2048, countGrowth: 2,
            currentSize: 4096, currentCount: 4, confidence: 'high', reason: HOSTILE,
        }],
    };
    const html = buildMemoryDashboardHtml(snapshot, diff);
    assert.ok(html.includes('Content-Security-Policy'), 'the dashboard must declare a CSP');
    assert.ok(/script-src 'nonce-[^']+'/u.test(html), 'the inline script must be nonce-gated');
    assert.ok(!html.includes('</script><img'), 'profiled-program data must not close the inline <script> element early');
    assert.ok(html.includes('escapeHtml(basename(a.file))'), 'allocation paths must be escaped before innerHTML');
    assert.ok(html.includes('escapeHtml(lk.reason)'), 'leak reasons must be escaped before innerHTML');
}

function heapProfileButtonsOnlyWithATrace(): void {
    const withTrace = buildMemoryDashboardHtml(dashboardSnapshot({ heapProfilePath: '/tmp/lspui-mem-1.heapprofile' }));
    assert.ok(withTrace.includes('Open Heap Profile in VS Code Viewer'), 'the dashboard must offer opening the native .heapprofile viewer');
    assert.ok(withTrace.includes('openHeapProfile'), 'the heap-profile button must post a message the extension handles');
    assert.ok(withTrace.includes('Open in Speedscope (external)'), 'the dashboard must offer the speedscope deep link (speedscope imports .heapprofile)');
    assert.ok(withTrace.includes('openSpeedscope'), 'the speedscope button must post a message the extension handles');
    // The action script always references the ids defensively; only the
    // BUTTON ELEMENTS (their visible labels) must be absent without a trace.
    const withoutTrace = buildMemoryDashboardHtml(dashboardSnapshot());
    assert.ok(!withoutTrace.includes('Open Heap Profile in VS Code Viewer'), 'no heap-profile button may render when no .heapprofile was written');
    assert.ok(!withoutTrace.includes('Open in Speedscope (external)'), 'no speedscope button may render when no .heapprofile was written');
}

function hostileRetentionGraphIsInert(): void {
    const result: ReferenceGraphResult = {
        targetType: HOSTILE, maxDepth: 5, maxNodes: 200, script: '',
        graph: {
            nodes: [{ id: 1, type: HOSTILE, size: 64, repr: HOSTILE, depth: 0, isTarget: true }],
            edges: [{ from: 1, to: 1, label: HOSTILE }], cycles: [], retentionPath: [HOSTILE],
        },
    };
    const html = buildRefGraphHtml(result);
    assert.ok(html.includes('Content-Security-Policy'), 'the retention graph must declare a CSP');
    assert.ok(/script-src 'nonce-[^']+'/u.test(html), 'the inline script must be nonce-gated');
    assert.ok(!html.includes('</script><img'), 'node reprs / retention path steps must not close the inline <script> early');
    assert.ok(html.includes('&lt;/script&gt;'), 'the target type in the heading must be HTML-escaped');
}

/** A static root holding an entry array that keeps two sessions alive. */
const ROOTED_SESSIONS: ReferenceGraphResult = {
    targetType: 'Shop.Session', maxDepth: 8, maxNodes: 60, script: '',
    graph: {
        nodes: [
            { id: 0, type: 'Shop.Session', size: 40, repr: '0x1', depth: 0, isTarget: true },
            { id: 1, type: 'Shop.Session', size: 40, repr: '0x2', depth: 0, isTarget: true },
            { id: 2, type: 'Entry[]', size: 4096, repr: '0x3', depth: 1, isTarget: false },
            { id: 3, type: 'Dictionary`2[Int32,Shop.Session]', size: 80, repr: 'static sessions', depth: 2, isTarget: false, isRoot: true, rootKind: 'Static' },
        ],
        edges: [{ from: 3, to: 2, label: '_entries' }, { from: 2, to: 0, label: '[0]' }, { from: 2, to: 1, label: '[1]' }],
        cycles: [],
    },
};

// [PROF-UI-REFGRAPH]: a root is named by its label and drawn apart from the
// objects it holds, so a module value or a static field reads as written.
function rootsAreNamedAboveWhatTheyHold(): void {
    const texts = drawnTexts(buildRefGraphHtml(ROOTED_SESSIONS));
    const drawn = (text: string): DrawnText | undefined => texts.find((item) => item.text === text);
    const root = drawn('static sessions');
    const holder = drawn('Entry[]');
    const target = drawn('Shop.Session');
    assert.ok(root !== undefined, `the root is named by its label: ${JSON.stringify(texts.map((item) => item.text))}`);
    assert.ok(drawn('Dictionary`2[Int32,Shop.Session]') !== undefined, 'the root still shows its type');
    assert.ok(holder !== undefined && target !== undefined, 'the holder and the targets are labelled');
    assert.ok(root.y < holder.y && holder.y < target.y, 'roots sit above their holders, and holders above the targets');
    assert.notStrictEqual(root.fill, holder.fill, 'a root is drawn apart from an intermediate holder');
}

function decodingKeepsRoots(): void {
    const graph = decodeRefGraph({
        nodes: [
            { id: 3, type: 'Dictionary', size: 80, repr: 'static sessions', depth: 2, isTarget: false, isRoot: true, rootKind: 'Static' },
            { id: 2, type: 'Entry[]', size: 4096, repr: '0x3', depth: 1, isTarget: false, isRoot: false },
        ],
        edges: [],
        cycles: [],
    });
    const nodes = graph?.nodes ?? [];
    const root = nodes.at(0);
    const holder = nodes.at(1);
    assert.strictEqual(root?.isRoot, true, 'the root survives decoding');
    assert.strictEqual(root.rootKind, 'Static', 'and so does its kind');
    assert.strictEqual(root.repr, 'static sessions', 'and its label');
    assert.strictEqual(holder?.isRoot, false, 'a holder is not a root');
}

function anEmptyWalkSaysSo(): void {
    const html = buildRefGraphHtml({ targetType: 'Widget', maxDepth: 5, maxNodes: 200, script: '', graph: undefined });
    assert.ok(html.includes('No reference graph data available'), 'an empty walk must say so, never show a blank canvas');
}

function viewsFollowTheEditorTheme(): void {
    const surfaces = [
        buildMemoryDashboardHtml(dashboardSnapshot()),
        buildRefGraphHtml({ targetType: 'W', maxDepth: 5, maxNodes: 200, script: '' }),
        buildFlamegraphHtml(profileResult()),
    ];
    for (const html of surfaces) {
        assert.ok(html.includes('var(--vscode-editor-background'), 'panel background must track the active VS Code theme');
        assert.ok(html.includes('var(--vscode-editor-foreground'), 'panel text must track the active VS Code theme');
    }
}

function embeddedJsonStaysInert(): void {
    const embedded = embedJson({ name: HOSTILE });
    assert.ok(!embedded.includes('</script>'), 'embedJson must escape < so the script cannot be closed');
    const roundTripped: unknown = JSON.parse(embedded);
    assert.strictEqual(stringField(roundTripped, 'name'), HOSTILE, 'escaping must not corrupt the payload');
}

suite('Profiler webviews — shared host hardening', () => {
    test('memory dashboard HTML is CSP-locked and hostile allocation data cannot escape the script', hostileDashboardIsInert);
    // [PROF-CPU-START] The dashboard is the landing view for memory results;
    // the raw V8 .heapprofile opens on demand from its own button, and no
    // button is rendered when no trace was written.
    test('the dashboard offers the raw heap profile via a button only when one exists', heapProfileButtonsOnlyWithATrace);
    test('retention graph HTML is CSP-locked and hostile type names/reprs cannot escape', hostileRetentionGraphIsInert);
    test('retention graph names each root by its label, above the holders and the targets they keep alive', rootsAreNamedAboveWhatTheyHold);
    test('a decoded retention graph keeps each root, its kind and its label', decodingKeepsRoots);
    test('retention graph renders an honest empty state when the walk found nothing', anEmptyWalkSaysSo);
    test('profiler webviews follow the editor theme instead of hardcoding a dark palette', viewsFollowTheEditorTheme);
    test('embedJson keeps a </script> payload inert inside an inline script', embeddedJsonStaysInert);
});
