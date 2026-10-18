// Tests for [PROF-UI-STATE]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * PROGRESS OPERATION LOG — the e2e seam every host's suites read. A host runs
 * many suites in one extension host, so one run records hundreds of
 * operations. A log that trimmed single entries at a small cap stopped being a
 * cursor (its length froze at the cap, so `slice(before)` saw nothing) and
 * started to lie (a trimmed `begin:` left its `end:` orphaned).
 */

import * as assert from 'assert';
import { recordedOperations, withUserProgress } from '../../progress-ops';
import { hostApi } from './test-helpers';

/** More operations than a host's whole profiler run records, each with a step. */
const LONG_RUN = 600;

function balance(entries: readonly string[]): number {
    return entries.filter((e) => e.startsWith('begin:')).length - entries.filter((e) => e.startsWith('end:')).length;
}

suite('Progress operation log — a long run keeps an honest log', () => {
    suiteSetup(async () => { await hostApi(); });

    test('every operation of a long run stays recorded, balanced, with its length a cursor', async () => {
        const start = recordedOperations().length;
        for (let index = 0; index < LONG_RUN; index += 1) {
            await withUserProgress(`long run ${index}`, async (report) => { report('working'); });
        }
        assert.strictEqual(recordedOperations().length, start + LONG_RUN * 3, 'begin, step and end of every operation are kept');
        assert.ok(recordedOperations().includes('begin:long run 0'), 'the first operation of the run is still there');
        const before = recordedOperations().length;
        await withUserProgress('probe', async () => undefined);
        assert.deepStrictEqual(recordedOperations().slice(before), ['begin:probe', 'end:probe'], 'the length is a cursor');
        assert.strictEqual(balance(recordedOperations()), 0, 'every begin in the log has its end');
    });
});
