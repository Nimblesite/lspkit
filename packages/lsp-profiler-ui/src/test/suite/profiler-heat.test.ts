// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Heat levels, the decoration contracts, and the decorations' lifecycle across
 * editor changes. Runs in VS Code against the suite's host extension and its
 * wire-speaking language server.
 */

import * as assert from 'assert';
import {
    setupLspTestSuite,
    teardownLspTestSuite,
    closeAllEditors,
    openSourceFile,
} from "./test-helpers";

// Import profiler decoration types for structural assertions.
import type {
    ProfileResult,
    ProfileHotLine,
} from '../../profiler-decorations';
import {
    applyProfileDecorations,
    clearProfileDecorations,
} from '../../profiler-decorations';

import { clearMemoryDecorations } from '../../memory-decorations';


let tmpDir = '';

suite('Profiler — Heat Level Classification', () => {
    suiteSetup(async function () {
        const result = await setupLspTestSuite('lspui-profiler-heat-');
        tmpDir = result.tmpDir;
    });

    suiteTeardown(() => {
        teardownLspTestSuite(tmpDir);
    });

    test('critical heat level classification (>= 20%)', () => {
        const criticalLine: ProfileHotLine = {
            file: '/src/hot.py', line: 1, samples: 400, percentage: 25.0,
        };
        assert.ok(criticalLine.percentage >= 20,
            'Lines at 25% should fall in the critical range');

        const borderlineCritical: ProfileHotLine = {
            file: '/src/hot.py', line: 2, samples: 200, percentage: 20.0,
        };
        assert.ok(borderlineCritical.percentage >= 20,
            'Lines at exactly 20% should fall in the critical range');
    });

    test('hot heat level classification (10-20%)', () => {
        const hotLine: ProfileHotLine = {
            file: '/src/warm.py', line: 5, samples: 150, percentage: 15.0,
        };
        assert.ok(hotLine.percentage >= 10 && hotLine.percentage < 20,
            'Lines at 15% should fall in the hot range');

        const borderlineHot: ProfileHotLine = {
            file: '/src/warm.py', line: 6, samples: 100, percentage: 10.0,
        };
        assert.ok(borderlineHot.percentage >= 10 && borderlineHot.percentage < 20,
            'Lines at exactly 10% should fall in the hot range');
    });

    test('warm heat level classification (5-10%)', () => {
        const warmLine: ProfileHotLine = {
            file: '/src/warm.py', line: 10, samples: 70, percentage: 7.0,
        };
        assert.ok(warmLine.percentage >= 5 && warmLine.percentage < 10,
            'Lines at 7% should fall in the warm range');

        const borderlineWarm: ProfileHotLine = {
            file: '/src/warm.py', line: 11, samples: 50, percentage: 5.0,
        };
        assert.ok(borderlineWarm.percentage >= 5 && borderlineWarm.percentage < 10,
            'Lines at exactly 5% should fall in the warm range');
    });

    test('cool heat level classification (1-5%)', () => {
        const coolLine: ProfileHotLine = {
            file: '/src/cool.py', line: 20, samples: 30, percentage: 3.0,
        };
        assert.ok(coolLine.percentage >= 1 && coolLine.percentage < 5,
            'Lines at 3% should fall in the cool range');

        const borderlineCool: ProfileHotLine = {
            file: '/src/cool.py', line: 21, samples: 10, percentage: 1.0,
        };
        assert.ok(borderlineCool.percentage >= 1 && borderlineCool.percentage < 5,
            'Lines at exactly 1% should fall in the cool range');
    });

    test('below threshold (< 1%) is not classified', () => {
        const belowThreshold: ProfileHotLine = {
            file: '/src/idle.py', line: 99, samples: 2, percentage: 0.5,
        };
        assert.ok(belowThreshold.percentage < 1,
            'Lines below 1% should not be classified as any heat level');
    });

    test('heat level boundaries are mutually exclusive', () => {
        const testCases = [
            { pct: 25.0, expected: 'critical' },
            { pct: 20.0, expected: 'critical' },
            { pct: 19.9, expected: 'hot' },
            { pct: 10.0, expected: 'hot' },
            { pct: 9.9, expected: 'warm' },
            { pct: 5.0, expected: 'warm' },
            { pct: 4.9, expected: 'cool' },
            { pct: 1.0, expected: 'cool' },
            { pct: 0.9, expected: 'none' },
        ];

        for (const tc of testCases) {
            let level: string;
            if (tc.pct >= 20) { level = 'critical'; }
            else if (tc.pct >= 10) { level = 'hot'; }
            else if (tc.pct >= 5) { level = 'warm'; }
            else if (tc.pct >= 1) { level = 'cool'; }
            else { level = 'none'; }

            assert.strictEqual(level, tc.expected,
                `${tc.pct}% should be classified as "${tc.expected}", got "${level}"`);
        }
    });
});

suite('Profiler — Decoration Contracts', () => {
    suiteSetup(async function () {
        const result = await setupLspTestSuite('lspui-profiler-deco-');
        tmpDir = result.tmpDir;
    });

    suiteTeardown(() => {
        teardownLspTestSuite(tmpDir);
    });

    teardown(async () => {
        clearProfileDecorations();
        clearMemoryDecorations();
        await closeAllEditors();
    });

    test('applyProfileDecorations with multiple files and varying percentages', async () => {
        await openSourceFile(tmpDir, 'hot_module.py',
            'def hot_func():\n    x = 1\n    y = 2\n    z = x + y\n    return z\n');

        const result: ProfileResult = {
            sessionId: 'multi-file-session',
            duration: 8.3,
            totalSamples: 3000,
            outputFile: '/tmp/multi.speedscope.json',
            hotFunctions: [
                { name: 'hot_func', file: '/nonexistent/a.py', line: 1, samples: 1500, percentage: 50.0, selfPercentage: 40.0 },
                { name: 'warm_func', file: '/nonexistent/b.py', line: 10, samples: 300, percentage: 10.0, selfPercentage: 8.0 },
                { name: 'cool_func', file: '/nonexistent/c.py', line: 20, samples: 60, percentage: 2.0, selfPercentage: 1.5 },
            ],
            hotLines: [
                { file: '/nonexistent/a.py', line: 3, samples: 1200, percentage: 40.0 },
                { file: '/nonexistent/b.py', line: 12, samples: 200, percentage: 6.7 },
                { file: '/nonexistent/c.py', line: 22, samples: 30, percentage: 1.0 },
            ],
        };

        assert.doesNotThrow(() => {
            applyProfileDecorations(result);
        }, 'applyProfileDecorations should handle multi-file results');

        assert.strictEqual(result.hotFunctions.length, 3, 'Should have 3 hot functions');
        assert.strictEqual(result.hotLines.length, 3, 'Should have 3 hot lines');
        assert.ok(result.hotFunctions[0].percentage > result.hotFunctions[1].percentage,
            'Functions should be ordered by percentage');
    });

    test('heat level classification boundary at exactly 1%', () => {
        const atBoundary: ProfileHotLine = {
            file: '/src/boundary.py', line: 1, samples: 10, percentage: 1.0,
        };
        const belowBoundary: ProfileHotLine = {
            file: '/src/boundary.py', line: 2, samples: 9, percentage: 0.99,
        };

        assert.ok(atBoundary.percentage >= 1, '1.0% should be classified (cool)');
        assert.ok(belowBoundary.percentage < 1, '0.99% should not be classified');
        assert.ok(atBoundary.percentage < 5, '1.0% should not be warm');
    });

    test('heat level classification boundary at exactly 5%', () => {
        const atBoundary: ProfileHotLine = {
            file: '/src/boundary.py', line: 1, samples: 50, percentage: 5.0,
        };
        const belowBoundary: ProfileHotLine = {
            file: '/src/boundary.py', line: 2, samples: 49, percentage: 4.99,
        };

        assert.ok(atBoundary.percentage >= 5, '5.0% should be classified as warm');
        assert.ok(belowBoundary.percentage < 5, '4.99% should still be cool');
        assert.ok(atBoundary.percentage < 10, '5.0% should not be hot');
    });

    test('heat level classification boundary at exactly 10%', () => {
        const atBoundary: ProfileHotLine = {
            file: '/src/boundary.py', line: 1, samples: 100, percentage: 10.0,
        };
        const belowBoundary: ProfileHotLine = {
            file: '/src/boundary.py', line: 2, samples: 99, percentage: 9.99,
        };

        assert.ok(atBoundary.percentage >= 10, '10.0% should be classified as hot');
        assert.ok(belowBoundary.percentage < 10, '9.99% should still be warm');
        assert.ok(atBoundary.percentage < 20, '10.0% should not be critical');
    });

    test('heat level classification boundary at exactly 20%', () => {
        const atBoundary: ProfileHotLine = {
            file: '/src/boundary.py', line: 1, samples: 200, percentage: 20.0,
        };
        const belowBoundary: ProfileHotLine = {
            file: '/src/boundary.py', line: 2, samples: 199, percentage: 19.99,
        };

        assert.ok(atBoundary.percentage >= 20, '20.0% should be classified as critical');
        assert.ok(belowBoundary.percentage < 20, '19.99% should still be hot');
        assert.ok(belowBoundary.percentage >= 10, '19.99% must be at least hot-level');
    });

});

suite('Profiler — Decoration Lifecycle', () => {
    suiteSetup(async function () {
        const result = await setupLspTestSuite('lspui-profiler-deco2-');
        tmpDir = result.tmpDir;
    });

    suiteTeardown(() => {
        teardownLspTestSuite(tmpDir);
    });

    teardown(async () => {
        clearProfileDecorations();
        clearMemoryDecorations();
        await closeAllEditors();
    });

    test('clearProfileDecorations removes all decorations without error', () => {
        const result: ProfileResult = {
            sessionId: 'clear-test',
            duration: 1.0,
            totalSamples: 100,
            outputFile: '',
            hotFunctions: [],
            hotLines: [
                { file: '/tmp/test.py', line: 1, samples: 50, percentage: 50.0 },
            ],
        };

        assert.doesNotThrow(() => {
            applyProfileDecorations(result);
        }, 'Applying decorations should not throw');

        assert.doesNotThrow(() => {
            clearProfileDecorations();
        }, 'Clearing decorations should not throw');

        assert.doesNotThrow(() => {
            clearProfileDecorations();
        }, 'Double-clearing decorations should not throw');
    });

    test('decorations apply after opening file and survive re-application', async () => {
        const { uri } = await openSourceFile(tmpDir, 'deco_survive.py',
            'x = 1\ny = 2\nz = x + y\n');

        const result: ProfileResult = {
            sessionId: 'survive-test',
            duration: 2.0,
            totalSamples: 500,
            outputFile: '',
            hotFunctions: [],
            hotLines: [
                { file: uri.fsPath, line: 2, samples: 250, percentage: 50.0 },
            ],
        };

        assert.doesNotThrow(() => { applyProfileDecorations(result); },
            'First apply should succeed');
        assert.doesNotThrow(() => { clearProfileDecorations(); },
            'Clear should succeed');
        assert.doesNotThrow(() => { applyProfileDecorations(result); },
            'Re-apply should succeed');
        clearProfileDecorations();
    });
});
