// Tests for [PROF-UI] + [PROF-DOTNET-CPU-SYMBOLS]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * CPU UI, RUNTIME-NEUTRAL: a hot function the runtime could not place, such as
 * a framework frame (`System.Threading.Thread.StartCallback()`) whose PDB is not
 * on this machine, reaches the screens by name only. The adapter used to turn
 * the absent location into `""` and line 0, which the results table rendered
 * as a `:0` link to nowhere.
 */

import * as assert from 'assert';
import { toProfileResult } from '../../wire-adapters';
import { buildFlamegraphHtml } from '../../profiler-flamegraph-html';
import type { ProfileReport } from '../../wire-generated';
import { hostApi } from './test-helpers';

const FRAMEWORK_FRAME = 'System.Threading.Thread.StartCallback()';

function report(): ProfileReport {
    return {
        session_id: 'cpu-1', pid: 7, runtime: 'dotnet', duration_secs: 3, total_samples: 300,
        user_samples: 290, sample_rate: 100,
        hot_functions: [
            { name: FRAMEWORK_FRAME, samples: 290, percentage: 100, self_samples: 0, self_percentage: 0 },
            { name: 'App.Hot.spin', file: '/src/Program.fs', line: 26, samples: 280, percentage: 96.5, self_samples: 280, self_percentage: 96.5 },
        ],
        hot_lines: [{ file: '/src/Program.fs', line: 29, samples: 200, percentage: 69 }],
        artifacts: {},
        diagnostics: [],
    };
}

suite('CPU UI — a hot function with no location', () => {
    suiteSetup(async () => { await hostApi(); });

    test('reaches the results by name, with no invented file or line', () => {
        const result = toProfileResult(report());
        const [framework, spin] = result.hotFunctions;
        assert.strictEqual(framework?.name, FRAMEWORK_FRAME, 'the unlocated frame is still listed');
        assert.strictEqual(framework?.file, undefined, 'no file is invented');
        assert.strictEqual(framework?.line, undefined, 'no line 0 is invented');
        assert.strictEqual(framework?.percentage, 100);
        assert.deepStrictEqual([spin?.file, spin?.line], ['/src/Program.fs', 26], 'a located frame keeps its anchor');
    });

    test('the results table embeds the unlocated frame without a location', () => {
        const html = buildFlamegraphHtml(toProfileResult(report()));
        assert.ok(html.includes(FRAMEWORK_FRAME), 'the frame is in the table data');
        assert.ok(!html.includes('"file":""'), 'no empty file is embedded');
        assert.ok(!html.includes('"line":0'), 'no line 0 is embedded');
        assert.ok(html.includes('"file":"/src/Program.fs"'), 'a located frame keeps its file');
    });
});
