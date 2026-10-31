// Tests for [PROF-UI-MEMORY] + [PROF-UI-REFGRAPH]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * MEMORY UI, RUNTIME-NEUTRAL — what the memory screens show comes from what the
 * runtime reported, in terms every runtime shares. The library was extracted
 * from a Python-only host: its reference picker offered Python builtins and
 * source-text class names (which a .NET heap never matches), its dashboard
 * dropped the runtime's type names, and its GC toast read a runtime-defined
 * counter as "objects freed". Each test here fails on that behaviour.
 */

import * as assert from 'node:assert';
import { toMemoryIngestResult } from '../../wire-adapters';
import { toDashboardDiff, toDashboardSnapshot } from '../../memory-dashboard-mapping';
import type { MemoryDashboardSnapshot } from '../../memory-dashboard';
import { buildMemoryDashboardHtml } from '../../memory-dashboard';
import { referenceTypeCandidates } from '../../memory-ref-picker';
import { gcSummary } from '../../memory-profiler';
import type { AllocationSite, MemoryResult } from '../../wire-generated';
import { hostApi } from './test-helpers';

function snapshotResult(sites: Omit<AllocationSite, 'traceback'>[]): MemoryResult {
    return {
        memory_session_id: 'm-1',
        outcome: {
            kind: 'Snapshot',
            snapshot: {
                snapshot_id: 'snap-1', current_bytes: 10, peak_bytes: 10, object_count: 3,
                gc_generations: [0, 0, 0], sites: sites.map((site) => ({ ...site, traceback: [] })),
            },
            timeline: [],
            trending_upward: false,
        },
        diagnostics: [],
    };
}

function dashboard(topAllocations: MemoryDashboardSnapshot['topAllocations']): MemoryDashboardSnapshot {
    return {
        memorySessionId: 'm-1', snapshotId: 'snap-1', currentMemory: 1, peakMemory: 1, gcObjects: 1,
        gcCounts: [0, 0, 0], topAllocations, timeline: [], heapProfilePath: '',
    };
}

suite('Memory UI — runtime-reported type names survive to the screen', () => {
    suiteSetup(async () => { await hostApi(); });

    test('a snapshot site keeps the runtime\'s exact type name', () => {
        const snap = toDashboardSnapshot(toMemoryIngestResult(snapshotResult([
            { type_name: 'App.Orders.LeakedPayload', file: '/src/Program.cs', line: 12, size_bytes: 4096, count: 8 },
        ])));
        assert.strictEqual(snap.topAllocations[0]?.typeName, 'App.Orders.LeakedPayload');
    });

    test('a line-attributed site without a type carries no invented type name', () => {
        const snap = toDashboardSnapshot(toMemoryIngestResult(snapshotResult([
            { file: '/src/main.py', line: 3, size_bytes: 64, count: 1 },
        ])));
        assert.strictEqual(snap.topAllocations[0]?.typeName, undefined);
        assert.strictEqual(snap.topAllocations[0]?.file, '/src/main.py');
    });

    // [PROF-DOTNET-SOURCE] + [PROF-WIRE]: an array, or a framework type, has no
    // location. It still appears in the UI, by name only: no `""` file, no line 0.
    test('a site with no location reaches the dashboard by name, with no invented file or line', () => {
        const snap = toDashboardSnapshot(toMemoryIngestResult(snapshotResult([
            { type_name: 'App.Order[]', size_bytes: 8192, count: 2 },
            { type_name: 'App.Order', file: '/src/Ledger.fs', line: 21, size_bytes: 4096, count: 64 },
        ])));
        const [array, order] = snap.topAllocations;
        assert.strictEqual(array?.typeName, 'App.Order[]', 'the unlocated site is still listed');
        assert.strictEqual(array?.file, undefined, 'no file is invented');
        assert.strictEqual(array?.line, undefined, 'no line 0 is invented');
        assert.strictEqual(array?.size, 8192);
        assert.deepStrictEqual([order?.file, order?.line], ['/src/Ledger.fs', 21], 'a located site keeps its anchor');
    });

    test('a suspected leak keeps the runtime\'s type name', () => {
        const diff = toDashboardDiff({
            kind: 'diff',
            suspectedLeaks: [{ file: '', line: 0, typeName: 'App.Cache', sizeGrowth: 10, countGrowth: 1, currentSize: 10, currentCount: 1, confidence: 'HIGH', reason: 'grew' }],
        });
        assert.strictEqual(diff.suspectedLeaks[0]?.typeName, 'App.Cache');
    });

    test('the dashboard embeds the type name with its site', () => {
        const html = buildMemoryDashboardHtml(dashboard([
            { file: '/src/Program.cs', line: 12, size: 4096, count: 8, typeName: 'App.Orders.LeakedPayload' },
        ]));
        assert.ok(html.includes('App.Orders.LeakedPayload'), 'the type is in the dashboard data');
        assert.ok(html.includes('function siteLabel('), 'rows label sites by type and location');
    });
});

suite('Memory UI — the reference picker offers what the runtime reported', () => {
    test('candidates are the snapshot\'s distinct type names, in snapshot order', () => {
        const candidates = referenceTypeCandidates(dashboard([
            { file: 'a', line: 1, size: 300, count: 3, typeName: 'App.Big' },
            { file: 'b', line: 2, size: 200, count: 2, typeName: 'System.String' },
            { file: 'c', line: 3, size: 100, count: 1, typeName: 'App.Big' },
        ]));
        assert.deepStrictEqual(candidates, ['App.Big', 'System.String']);
    });

    test('no snapshot means no guessed candidates — never a language\'s builtins', () => {
        assert.deepStrictEqual(referenceTypeCandidates(undefined), []);
        const lineOnly = referenceTypeCandidates(dashboard([{ file: 'a', line: 1, size: 1, count: 1 }]));
        for (const builtin of ['dict', 'list', 'set', 'tuple', 'frozenset']) {
            assert.ok(!lineOnly.includes(builtin), `${builtin} is not offered`);
        }
        assert.deepStrictEqual(lineOnly, [], 'a line-only snapshot offers no invented types');
    });
});

suite('Memory UI — the GC toast speaks every runtime\'s language', () => {
    suiteSetup(async () => { await hostApi(); });

    test('freed bytes are reported; the runtime-defined counter is never called an object count', () => {
        const text = gcSummary({ kind: 'gc', collected: 1, uncollectable: 0, freedBytes: 2 * 1_048_576 });
        assert.strictEqual(text, 'LspUi: Garbage collection ran — freed 2.0 MB');
        assert.ok(!text.includes('gc.collect()'), 'no Python call spelling');
        assert.ok(!text.includes('1 object'), 'collected is not an object count');
    });

    test('uncollectable objects are surfaced when the runtime reports them', () => {
        const text = gcSummary({ kind: 'gc', collected: 40, uncollectable: 3, freedBytes: 512 });
        assert.strictEqual(text, 'LspUi: Garbage collection ran — freed 512 B; 3 object(s) could not be collected');
    });

    test('a heap that grew across the collection never reports negative bytes freed', () => {
        assert.strictEqual(
            gcSummary({ kind: 'gc', collected: 1, uncollectable: 0, freedBytes: -100 }),
            'LspUi: Garbage collection ran — freed 0 B',
        );
    });
});
