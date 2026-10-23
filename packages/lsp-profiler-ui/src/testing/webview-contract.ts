import { profileVariable, selectedProfileVariable } from '../debug-selection';
import { dashboardSnapshot, assertHardened } from './webview-contract-helpers';
// Implements [PROF-UI-WEBVIEW] + [PROF-EXPORT-FLAMEGRAPH].
// See docs/specs/LSP-PROFILER-SPEC.md
/**
 * PROFILER WEBVIEW HOST — hardening and honesty of every profiler results
 * panel (CPU results, memory dashboard, retention graph).
 *
 * Why this suite exists: before the shared host, the memory dashboard and the
 * retention graph shipped with NO Content-Security-Policy, embedded
 * profiled-program data (allocation paths, type reprs, leak reasons) into
 * their inline <script> without escaping `<` (a hostile `</script>` payload
 * broke out of the script element), and re-registered their message handler on
 * EVERY open — with the autopilot re-rendering the dashboard on each debugger
 * pause, one row click navigated N times. These tests pin all three fixes on
 * the real builders and, for the handler, on a real live webview panel.
 */

import { delay } from '../timeouts';
import * as assert from 'assert';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import {
    buildWebviewDocument,
    embedJson,
    SingletonWebviewPanel,
    type WebviewMessage,
} from '../profiler-webview';
import {
    buildMemoryDashboardHtml,
    type MemoryDiffData,
} from '../memory-dashboard';
import { buildRefGraphHtml, decodeRefGraph, type ReferenceGraphResult } from '../memory-ref-graph';
import {
    buildFlamegraphHtml,
    loadFlamegraphSvgDataUri,
} from '../profiler-flamegraph-html';
import type { ProfileResult } from '../profiler-decorations';
import { stringField } from '../unknown-shape';
import { pollUntilResult, closeAllEditors, removeTestDir } from './webview-contract-helpers';

/** A payload that closes the surrounding <script> if embedding is unescaped. */
const HOSTILE = '</script><img src=x onerror=alert(1)>';

interface DrawnText {
    readonly text: string;
    readonly y: number;
    readonly fill: string;
}

/**
 * Run a retention graph's inline script against a recording canvas and return
 * every text it draws, with where and in which colour: the panel as a reader sees it.
 */
function drawnTexts(html: string): DrawnText[] {
    const script = /<script nonce="[^"]+">([\s\S]*)<\/script>/.exec(html)?.[1] ?? '';
    const texts: DrawnText[] = [];
    const pen: Record<string | symbol, unknown> = {
        fillText(text: string, _x: number, y: number) { texts.push({ text, y, fill: String(pen.fillStyle) }); },
    };
    const ctx = new Proxy(pen, { get: (target, key) => (key in target ? target[key] : () => undefined) });
    const canvas = { width: 800, height: 500, clientWidth: 900, parentElement: {}, style: {}, getContext: () => ctx };
    const document = { getElementById: () => canvas, documentElement: {}, createElement: () => ({ style: {} }), body: { appendChild: () => undefined } };
    new Function('document', 'getComputedStyle', 'window', script)(document, () => ({ getPropertyValue: () => '#888888' }), { addEventListener() {} });
    return texts;
}

function profileResult(overrides: Partial<ProfileResult> = {}): ProfileResult {
    return {
        sessionId: 's-1',
        duration: 2.5,
        totalSamples: 250,
        outputFile: '/tmp/profile.speedscope.json',
        hotFunctions: [
            { name: 'hot', file: '/app/main.py', line: 3, samples: 200, percentage: 80, selfPercentage: 75 },
        ],
        hotLines: [{ file: '/app/main.py', line: 5, samples: 180, percentage: 72 }],
        ...overrides,
    };
}

suite('Profiler webviews — shared host hardening', () => {
    test('memory dashboard HTML is CSP-locked and hostile allocation data cannot escape the script', () => {
        const snapshot = dashboardSnapshot({
            topAllocations: [{ file: HOSTILE, line: 1, size: 1024, count: 2 }],
        });
        const diff: MemoryDiffData = {
            totalGrowth: 2048,
            totalFreed: 0,
            netGrowth: 2048,
            suspectedLeaks: [{
                file: HOSTILE, line: 1, sizeGrowth: 2048, countGrowth: 2,
                currentSize: 4096, currentCount: 4, confidence: 'high', reason: HOSTILE,
            }],
            grownAllocations: [],
        };
        const html = buildMemoryDashboardHtml(snapshot, diff);
        assert.ok(html.includes('Content-Security-Policy'), 'the dashboard must declare a CSP');
        assert.ok(/script-src 'nonce-[^']+'/.test(html), 'the inline script must be nonce-gated');
        assert.ok(
            !html.includes('</script><img'),
            'profiled-program data must not close the inline <script> element early',
        );
        assert.ok(html.includes('escapeHtml(basename(a.file))'), 'allocation paths must be escaped before innerHTML');
        assert.ok(html.includes('escapeHtml(lk.reason)'), 'leak reasons must be escaped before innerHTML');
    });

    // [PROF-CPU-START] The dashboard is the landing view for memory results;
    // the raw V8 .heapprofile opens on demand from its own button, and no
    // button is rendered when no trace was written.
    test('the dashboard offers the raw heap profile via a button only when one exists', () => {
        const withTrace = buildMemoryDashboardHtml(
            dashboardSnapshot({ heapProfilePath: '/tmp/lspui-mem-1.heapprofile' }),
        );
        assert.ok(
            withTrace.includes('Open Heap Profile in VS Code Viewer'),
            'the dashboard must offer opening the native .heapprofile viewer',
        );
        assert.ok(
            withTrace.includes('openHeapProfile'),
            'the heap-profile button must post a message the extension handles',
        );
        assert.ok(
            withTrace.includes('Open in Speedscope (external)'),
            'the dashboard must offer the speedscope deep link (speedscope imports .heapprofile)',
        );
        assert.ok(
            withTrace.includes('openSpeedscope'),
            'the speedscope button must post a message the extension handles',
        );

        // The action script always references the ids defensively; only the
        // BUTTON ELEMENTS (their visible labels) must be absent without a trace.
        const withoutTrace = buildMemoryDashboardHtml(dashboardSnapshot());
        assert.ok(
            !withoutTrace.includes('Open Heap Profile in VS Code Viewer'),
            'no heap-profile button may render when no .heapprofile was written',
        );
        assert.ok(
            !withoutTrace.includes('Open in Speedscope (external)'),
            'no speedscope button may render when no .heapprofile was written',
        );
    });

    test('retention graph HTML is CSP-locked and hostile type names/reprs cannot escape', () => {
        const result: ReferenceGraphResult = {
            targetType: HOSTILE,
            maxDepth: 5,
            maxNodes: 200,
            script: '',
            graph: {
                nodes: [{ id: 1, type: HOSTILE, size: 64, repr: HOSTILE, depth: 0, isTarget: true }],
                edges: [{ from: 1, to: 1, label: HOSTILE }],
                cycles: [],
                retentionPath: [HOSTILE],
            },
        };
        const html = buildRefGraphHtml(result);
        assert.ok(html.includes('Content-Security-Policy'), 'the retention graph must declare a CSP');
        assert.ok(/script-src 'nonce-[^']+'/.test(html), 'the inline script must be nonce-gated');
        assert.ok(
            !html.includes('</script><img'),
            'node reprs / retention path steps must not close the inline <script> early',
        );
        assert.ok(html.includes('&lt;/script&gt;'), 'the target type in the heading must be HTML-escaped');
    });

    // [PROF-UI-REFGRAPH]: a root is named by its label and drawn apart from the
    // objects it holds, so a module value or a static field reads as written.
    test('retention graph names each root by its label, above the holders and the targets they keep alive', () => {
        const html = buildRefGraphHtml({
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
        });
        const texts = drawnTexts(html);
        const drawn = (text: string) => texts.find((item) => item.text === text);
        const root = drawn('static sessions');
        const holder = drawn('Entry[]');
        const target = drawn('Shop.Session');
        assert.ok(root, `the root is named by its label: ${JSON.stringify(texts.map((item) => item.text))}`);
        assert.ok(drawn('Dictionary`2[Int32,Shop.Session]'), 'the root still shows its type');
        assert.ok(holder && target, 'the holder and the targets are labelled');
        assert.ok(root.y < holder.y && holder.y < target.y, 'roots sit above their holders, and holders above the targets');
        assert.notStrictEqual(root.fill, holder.fill, 'a root is drawn apart from an intermediate holder');
    });

    test('a decoded retention graph keeps each root, its kind and its label', () => {
        const graph = decodeRefGraph({
            nodes: [
                { id: 3, type: 'Dictionary', size: 80, repr: 'static sessions', depth: 2, isTarget: false, isRoot: true, rootKind: 'Static' },
                { id: 2, type: 'Entry[]', size: 4096, repr: '0x3', depth: 1, isTarget: false, isRoot: false },
            ],
            edges: [],
            cycles: [],
        });
        const [root, holder] = graph?.nodes ?? [];
        assert.strictEqual(root?.isRoot, true, 'the root survives decoding');
        assert.strictEqual(root?.rootKind, 'Static', 'and so does its kind');
        assert.strictEqual(root?.repr, 'static sessions', 'and its label');
        assert.strictEqual(holder?.isRoot, false, 'a holder is not a root');
    });

    test('retention graph renders an honest empty state when the walk found nothing', () => {
        const html = buildRefGraphHtml({
            targetType: 'Widget', maxDepth: 5, maxNodes: 200, script: '', graph: undefined,
        });
        assert.ok(
            html.includes('No reference graph data available'),
            'an empty walk must say so, never show a blank canvas',
        );
    });

    test('profiler webviews follow the editor theme instead of hardcoding a dark palette', () => {
        const surfaces = [
            buildMemoryDashboardHtml(dashboardSnapshot()),
            buildRefGraphHtml({ targetType: 'W', maxDepth: 5, maxNodes: 200, script: '' }),
            buildFlamegraphHtml(profileResult()),
        ];
        for (const html of surfaces) {
            assert.ok(
                html.includes('var(--vscode-editor-background'),
                'panel background must track the active VS Code theme',
            );
            assert.ok(
                html.includes('var(--vscode-editor-foreground'),
                'panel text must track the active VS Code theme',
            );
        }
    });

    test('embedJson keeps a </script> payload inert inside an inline script', () => {
        const embedded = embedJson({ name: HOSTILE });
        assert.ok(!embedded.includes('</script>'), 'embedJson must escape < so the script cannot be closed');
        const roundTripped: unknown = JSON.parse(embedded);
        assert.strictEqual(stringField(roundTripped, 'name'), HOSTILE, 'escaping must not corrupt the payload');
    });
});

// Implements [PROF-EXPORT-FLAMEGRAPH]. See docs/specs/LSP-PROFILER-SPEC.md
suite('Profiler webviews — flame graph hero', () => {
    let tmpDir = '';

    suiteSetup(() => {
        tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'lspui-flame-hero-'));
    });

    suiteTeardown(() => {
        removeTestDir(tmpDir);
    });

    function writeSvg(name: string, contents: string): string {
        const svgPath = path.join(tmpDir, name);
        fs.writeFileSync(svgPath, contents);
        return svgPath;
    }

    test('the results panel embeds the LSP-exported flame graph SVG as its hero', () => {
        const svgPath = writeSvg('profile.svg', '<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>');
        const html = buildFlamegraphHtml(profileResult({ flamegraphPath: svgPath }));
        assert.ok(
            html.includes('data:image/svg+xml;base64,'),
            'the flame graph SVG must be inlined as a data URI (a profiler must show a flame graph)',
        );
        assert.ok(
            html.includes('open-flame-svg'),
            'the hero must offer opening the interactive SVG externally',
        );
        assert.ok(html.includes('openFlamegraphSvg'), 'the open action must post a message the extension handles');
    });

    // [PROF-CPU-START] The built-in `.cpuprofile` viewer is on-demand, not the
    // landing view — so the panel itself must carry the way into it. Without
    // this button the raw trace is only reachable through the (dismissable)
    // completion toast.
    test('the results panel offers the raw trace via an "Open Trace in VS Code Viewer" button', () => {
        const withTrace = buildFlamegraphHtml(
            profileResult({ cpuProfilePath: '/tmp/lspui-s-1.cpuprofile' }),
        );
        assert.ok(
            withTrace.includes('Open Trace in VS Code Viewer'),
            'the panel must offer opening the native .cpuprofile viewer',
        );
        assert.ok(
            withTrace.includes('openCpuProfile'),
            'the trace button must post a message the extension handles',
        );

        const withoutTrace = buildFlamegraphHtml(profileResult());
        assert.ok(
            !withoutTrace.includes('/tmp/lspui-s-1.cpuprofile'),
            'no stale trace path may be embedded when no .cpuprofile was produced',
        );
    });

    test('a missing or unreadable SVG degrades to the tables, never a broken image', () => {
        const withoutPath = buildFlamegraphHtml(profileResult());
        const withDeadPath = buildFlamegraphHtml(
            profileResult({ flamegraphPath: path.join(tmpDir, 'nope.svg') }),
        );
        for (const html of [withoutPath, withDeadPath]) {
            assert.ok(!html.includes('data:image/svg+xml'), 'no hero image without a readable SVG');
            assert.ok(!html.includes('<img'), 'no broken <img> element');
            assert.ok(html.includes('fn-body'), 'the hot-functions table must still render');
        }
    });

    test('loadFlamegraphSvgDataUri refuses empty and oversized artifacts', () => {
        assert.strictEqual(loadFlamegraphSvgDataUri(undefined), undefined);
        assert.strictEqual(loadFlamegraphSvgDataUri(''), undefined);
        const empty = writeSvg('empty.svg', '');
        assert.strictEqual(loadFlamegraphSvgDataUri(empty), undefined, 'an empty artifact is not a flame graph');
        const oversized = writeSvg('huge.svg', `<svg>${'x'.repeat(5 * 1024 * 1024)}</svg>`);
        assert.strictEqual(
            loadFlamegraphSvgDataUri(oversized), undefined,
            'an oversized artifact must not be inlined (it still opens externally)',
        );
    });
});

// Implements [PROF-UI-WEBVIEW] (once-bound message handler).
suite('Profiler webviews — singleton panel message handler', () => {
    teardown(async () => {
        await closeAllEditors();
    });

    test('re-opening a panel re-renders but never stacks a second message handler', async function () {
        this.timeout(30_000);
        const received: WebviewMessage[] = [];
        const panel = new SingletonWebviewPanel(() => 'lspui.test.handlerOnce', (msg) => {
            received.push(msg);
        });
        try {
            // Each render posts exactly one 'ready' message. With the pre-host
            // bug (a handler re-registered per open), the second render's single
            // post would be delivered TWICE — 3 messages total instead of 2.
            function doc(marker: string): string {
                return buildWebviewDocument({
                    title: 'handler-once probe',
                    css: '',
                    body: `<p>${marker}</p>`,
                    script: `acquireVsCodeApi().postMessage({ type: 'ready', file: '${marker}' });`,
                });
            }
            panel.show('probe', doc('first'));
            await pollUntilResult(
                async () => received.length,
                (count) => count >= 1,
            );
            panel.show('probe', doc('second'));
            await pollUntilResult(
                async () => received.length,
                (count) => count >= 2,
            );
            // Give a stacked handler time to double-deliver before asserting.
            await delay(1_000);
            assert.strictEqual(
                received.length,
                2,
                `each render must deliver its message exactly once, got ${received.length} ` +
                `(${received.map((m) => m.file ?? '?').join(', ')}) — a third delivery means stacked handlers`,
            );
            assert.ok(panel.isOpen(), 'the singleton panel must stay open across re-renders');
        } finally {
            panel.dispose();
        }
        assert.ok(!panel.isOpen(), 'dispose must settle the panel closed');
    });
});

// Implements [PROF-UI-DEBUG-LINK]. The same session boundary is tested in each VSIX.
suite('Profiler debugger selection contract', () => {
  test('preserves expressions and rejects a selection from another session', () => {
    const variable = { name: 'child', evaluateName: 'parent.child', value: '42', type: 'Record' };
    const selected = profileVariable({ sessionId: 'one', variable }, 'one');
    assert.equal(selected?.expression, 'parent.child');
    assert.equal(selected?.type, 'Record');
    assert.equal(selected?.value, '42');
    assert.equal(profileVariable({ sessionId: 'other', variable }, 'one'), undefined);
    assert.equal(profileVariable({ variable: { name: '', value: '' } }, 'one'), undefined);
  });
  test('debugger values cannot escape into executable webview content', () => {
    selectedProfileVariable.value = profileVariable({ name: HOSTILE, value: HOSTILE, type: HOSTILE }, 'one');
    try { assertHardened(buildFlamegraphHtml(profileResult()), 'debug variable'); }
    finally { selectedProfileVariable.value = undefined; }
  });
});
