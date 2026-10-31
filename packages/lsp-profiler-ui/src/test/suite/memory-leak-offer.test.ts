// Tests for [PROF-UI-AUTOPILOT]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * THE AUTOPILOT SAYS WHAT THE RUNTIME REPORTED. A .NET `byte[]` grown by user
 * code is a leak with a type and no source line of its own, and the offer named
 * every leak by `file:line`: SharpLsp's autopilot offered "Suspected memory
 * leak at :0 (HIGH)" on every run, because the type never reached the offer.
 * And the tracking toast told a .NET user to press Continue to capture each
 * pause, which a direct runtime never does. Each test here fails on that.
 */

import * as assert from 'node:assert';
import { describeLeak, isLocated, leakToOffer } from '../../memory-leak-offer';
import { stoppedWithoutSnapshot, trackingGuidance } from '../../memory-guidance';
import { toDiffResult } from '../../memory-dashboard-mapping';
import { toMemoryIngestResult } from '../../wire-adapters';
import type { SuspectedLeak } from '../../memory-decorations';
import type { MemoryResult, SuspectedLeak as WireLeak } from '../../wire-generated';

function leak(overrides: Partial<SuspectedLeak>): SuspectedLeak {
    return {
        file: '', line: 0, typeName: '', sizeGrowth: 1, countGrowth: 1, currentSize: 1,
        confidence: 'HIGH', reason: 'grew', ...overrides,
    };
}

/** The leaked payload: a type, and no line of its own. */
const PAYLOAD = leak({ typeName: 'System.Byte[]' });
/** The leaking user type, at its declaration. */
const WIDGET = leak({ file: '/src/LoopLeak/Program.cs', line: 4, typeName: 'LoopLeak.Widget' });

function wireDiff(leaks: Omit<WireLeak, 'traceback'>[]): MemoryResult {
    return {
        memory_session_id: 'm-1',
        outcome: {
            kind: 'Diff',
            diff: { total_growth: 1, total_freed: 0, net_growth: 1, grown: [], freed: [] },
            leaks: leaks.map((entry) => ({ ...entry, traceback: [] })),
        },
        diagnostics: [],
    };
}

suite('[PROF-UI-AUTOPILOT] the leak offer names what the runtime reported', () => {
    test('a diff keeps the type the runtime named, with or without a source line', () => {
        const shared = { size_growth: 64, count_growth: 1, current_size: 64, current_count: 1, reason: 'grew' };
        const diff = toDiffResult(toMemoryIngestResult(wireDiff([
            { ...shared, type_name: 'System.Byte[]', confidence: { kind: 'High' } },
            { ...shared, file: '/src/Program.cs', line: 4, type_name: 'LoopLeak.Widget', confidence: { kind: 'Medium' } },
            { ...shared, confidence: { kind: 'Low' } },
        ])));
        assert.deepStrictEqual(
            diff.suspectedLeaks.map(({ file, line, typeName, confidence }) => ({ file, line, typeName, confidence })),
            [
                { file: '', line: 0, typeName: 'System.Byte[]', confidence: 'HIGH' },
                { file: '/src/Program.cs', line: 4, typeName: 'LoopLeak.Widget', confidence: 'MEDIUM' },
                { file: '', line: 0, typeName: '', confidence: 'LOW' },
            ],
        );
    });

    test('a leak with a type and no line is named by its type, never ":0"', () => {
        assert.strictEqual(describeLeak(PAYLOAD), 'System.Byte[]');
        assert.ok(!describeLeak(PAYLOAD).includes(':'), describeLeak(PAYLOAD));
        assert.strictEqual(isLocated(PAYLOAD), false);
    });

    test('a leak with a type and a line is named by both, the file by its name alone', () => {
        assert.strictEqual(describeLeak(WIDGET), 'LoopLeak.Widget at Program.cs:4');
        assert.strictEqual(describeLeak({ ...WIDGET, typeName: '' }), 'Program.cs:4');
        assert.strictEqual(isLocated(WIDGET), true);
    });

    test('half a location is no location: a file without a line, or a line without a file', () => {
        for (const half of [leak({ file: '/src/Program.cs' }), leak({ line: 4 })]) {
            assert.strictEqual(isLocated(half), false, JSON.stringify(half));
            assert.strictEqual(describeLeak(half), 'an allocation the runtime did not name');
            assert.strictEqual(describeLeak({ ...half, typeName: 'App.Cache' }), 'App.Cache');
        }
    });

    test('among equally confident leaks the offer is the one with a source line, in either order', () => {
        assert.strictEqual(leakToOffer([PAYLOAD, WIDGET]), WIDGET);
        assert.strictEqual(leakToOffer([WIDGET, PAYLOAD]), WIDGET);
        const twin = leak({ typeName: 'System.String' });
        assert.strictEqual(leakToOffer([PAYLOAD, twin]), PAYLOAD, 'the first of true equals is kept');
    });

    test('confidence outranks a source line: the most confident leak is offered', () => {
        const mediumWidget = { ...WIDGET, confidence: 'MEDIUM' as const };
        assert.strictEqual(leakToOffer([mediumWidget, PAYLOAD]), PAYLOAD);
        const definite = leak({ typeName: 'App.Cycle', confidence: 'DEFINITE' });
        assert.strictEqual(leakToOffer([WIDGET, definite, PAYLOAD]), definite);
        assert.strictEqual(leakToOffer([]), undefined);
    });
});

suite('[PROF-UI-AUTOPILOT] the tracking toast says what the runtime does next', () => {
    test('a direct runtime is never told a pause is captured', () => {
        for (const interval of [undefined, 1, 30]) {
            const said = trackingGuidance(true, interval);
            assert.ok(said.includes('Memory tracking started.'), said);
            assert.ok(!said.includes('Continue'), `a direct runtime is never captured at a pause: ${said}`);
            assert.ok(said.includes('never while paused'), said);
            assert.ok(said.includes('its last snapshot opens'), said);
        }
    });

    test('a direct runtime is told its interval while interval capture is on, and to snapshot while it is off', () => {
        assert.ok(trackingGuidance(true, 1).includes('captured every 1 s while it runs'));
        assert.ok(trackingGuidance(true, 30).includes('captured every 30 s while it runs'));
        const off = trackingGuidance(true, undefined);
        assert.ok(off.includes('Take a snapshot while it runs'), off);
        assert.ok(!off.includes('captured every'), off);
    });

    test('a courier runtime is still told each pause is captured, whatever the interval', () => {
        for (const interval of [undefined, 30]) {
            const said = trackingGuidance(false, interval);
            assert.ok(said.includes('Press Continue to auto-capture each pause'), said);
            assert.ok(said.includes('automatic final snapshot'), said);
        }
    });

    test('stopped before any snapshot, a direct runtime is told to snapshot while it runs, a courier one while paused', () => {
        const direct = stoppedWithoutSnapshot(true);
        assert.ok(direct.includes('no snapshot was taken'), direct);
        assert.ok(direct.includes('Take a snapshot while the program runs, never while paused'), direct);
        assert.ok(!direct.includes('snapshot while paused'), `a paused direct runtime refuses a snapshot: ${direct}`);
        const courier = stoppedWithoutSnapshot(false);
        assert.ok(courier.includes('Take a snapshot while paused, or let the program finish'), courier);
    });
});
