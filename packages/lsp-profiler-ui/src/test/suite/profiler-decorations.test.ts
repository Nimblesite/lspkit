// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Profiler decorations: the CPU and memory decoration modules' shapes, and
 * applying and clearing them on open editors. Runs in VS Code against the
 * suite's host extension and its wire-speaking language server.
 */

import * as assert from 'assert';
import {
    setupLspTestSuite,
    teardownLspTestSuite,
    closeAllEditors,
} from "./test-helpers";

// Import profiler decoration types for structural assertions.
import type {
    ProfileResult,
    ProfileHotLine,
    ProfileHotFunction,
} from '../../profiler-decorations';
import {
    applyProfileDecorations,
    clearProfileDecorations,
} from '../../profiler-decorations';

// Import memory decoration types for structural assertions.
import type {
    MemoryAllocation,
    MemorySnapshotResult,
} from '../../memory-decorations';
import {
    applyMemoryDecorations,
    clearMemoryDecorations,
} from '../../memory-decorations';


let tmpDir = '';

suite('Profiler — Decoration Modules', () => {
    suiteSetup(async function () {
        const result = await setupLspTestSuite('lspui-profiler-dec-');
        tmpDir = result.tmpDir;
    });

    suiteTeardown(() => {
        teardownLspTestSuite(tmpDir);
    });

    teardown(async () => {
        await closeAllEditors();
    });

    test('profiler-decorations exports applyProfileDecorations function', () => {
        assert.strictEqual(typeof applyProfileDecorations, 'function',
            'applyProfileDecorations should be a function');
    });

    test('profiler-decorations exports clearProfileDecorations function', () => {
        assert.strictEqual(typeof clearProfileDecorations, 'function',
            'clearProfileDecorations should be a function');
    });

    test('memory-decorations exports applyMemoryDecorations function', () => {
        assert.strictEqual(typeof applyMemoryDecorations, 'function',
            'applyMemoryDecorations should be a function');
    });

    test('memory-decorations exports clearMemoryDecorations function', () => {
        assert.strictEqual(typeof clearMemoryDecorations, 'function',
            'clearMemoryDecorations should be a function');
    });

    test('clearProfileDecorations does not throw when no decorations exist', () => {
        assert.doesNotThrow(() => {
            clearProfileDecorations();
        }, 'clearProfileDecorations should be safe to call with no active decorations');
    });

    test('clearMemoryDecorations does not throw when no decorations exist', () => {
        assert.doesNotThrow(() => {
            clearMemoryDecorations();
        }, 'clearMemoryDecorations should be safe to call with no active decorations');
    });

    test('ProfileResult type has required fields', () => {
        const result: ProfileResult = {
            sessionId: 'test-session-001',
            duration: 5.2,
            totalSamples: 1000,
            outputFile: '/tmp/test.speedscope.json',
            hotFunctions: [],
            hotLines: [],
        };

        assert.strictEqual(result.sessionId, 'test-session-001');
        assert.strictEqual(result.duration, 5.2);
        assert.strictEqual(result.totalSamples, 1000);
        assert.strictEqual(result.outputFile, '/tmp/test.speedscope.json');
        assert.ok(Array.isArray(result.hotFunctions), 'hotFunctions should be an array');
        assert.ok(Array.isArray(result.hotLines), 'hotLines should be an array');
    });

    test('ProfileHotLine type has required fields', () => {
        const hotLine: ProfileHotLine = {
            file: '/src/app.py',
            line: 42,
            samples: 500,
            percentage: 25.0,
        };

        assert.strictEqual(hotLine.file, '/src/app.py');
        assert.strictEqual(hotLine.line, 42);
        assert.strictEqual(hotLine.samples, 500);
        assert.strictEqual(hotLine.percentage, 25.0);
    });

    test('ProfileHotFunction type has required fields', () => {
        const hotFunc: ProfileHotFunction = {
            name: 'process_data',
            file: '/src/pipeline.py',
            line: 15,
            samples: 800,
            percentage: 40.0,
            selfPercentage: 30.0,
        };

        assert.strictEqual(hotFunc.name, 'process_data');
        assert.strictEqual(hotFunc.file, '/src/pipeline.py');
        assert.strictEqual(hotFunc.line, 15);
        assert.strictEqual(hotFunc.samples, 800);
        assert.strictEqual(hotFunc.percentage, 40.0);
        assert.strictEqual(hotFunc.selfPercentage, 30.0);
    });

    test('MemoryAllocation type has required fields', () => {
        const alloc: MemoryAllocation = {
            file: '/src/data.py',
            line: 100,
            size: 10485760,
            count: 5000,
        };

        assert.strictEqual(alloc.file, '/src/data.py');
        assert.strictEqual(alloc.line, 100);
        assert.strictEqual(alloc.size, 10485760);
        assert.strictEqual(alloc.count, 5000);
    });

    test('MemorySnapshotResult type has required fields', () => {
        const snapshot: MemorySnapshotResult = {
            memorySessionId: 'mem-session-001',
            snapshotId: 'snap-001',
            currentMemory: 50000000,
            peakMemory: 75000000,
            topAllocations: [],
        };

        assert.strictEqual(snapshot.memorySessionId, 'mem-session-001');
        assert.strictEqual(snapshot.snapshotId, 'snap-001');
        assert.strictEqual(snapshot.currentMemory, 50000000);
        assert.strictEqual(snapshot.peakMemory, 75000000);
        assert.ok(Array.isArray(snapshot.topAllocations), 'topAllocations should be an array');
    });

});

suite('Profiler — Decoration Apply/Clear', () => {
    suiteSetup(async function () {
        const result = await setupLspTestSuite('lspui-profiler-dec2-');
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

    test('applyProfileDecorations handles empty result without throwing', () => {
        const emptyResult: ProfileResult = {
            sessionId: 'empty-session',
            duration: 0,
            totalSamples: 0,
            outputFile: '',
            hotFunctions: [],
            hotLines: [],
        };

        assert.doesNotThrow(() => {
            applyProfileDecorations(emptyResult);
        }, 'applyProfileDecorations should handle empty results gracefully');

        clearProfileDecorations();
    });

    test('applyMemoryDecorations handles empty result without throwing', () => {
        const emptySnapshot: MemorySnapshotResult = {
            memorySessionId: 'empty-mem',
            snapshotId: 'snap-empty',
            currentMemory: 0,
            peakMemory: 0,
            topAllocations: [],
        };

        assert.doesNotThrow(() => {
            applyMemoryDecorations(emptySnapshot);
        }, 'applyMemoryDecorations should handle empty snapshots gracefully');

        clearMemoryDecorations();
    });

    test('ProfileResult with populated hotFunctions validates structure', () => {
        const result: ProfileResult = {
            sessionId: 'populated-session',
            duration: 10.5,
            totalSamples: 5000,
            outputFile: '/tmp/profile.speedscope.json',
            hotFunctions: [
                {
                    name: 'compute',
                    file: '/src/math.py',
                    line: 10,
                    samples: 2500,
                    percentage: 50.0,
                    selfPercentage: 35.0,
                },
                {
                    name: 'transform',
                    file: '/src/utils.py',
                    line: 88,
                    samples: 1000,
                    percentage: 20.0,
                    selfPercentage: 15.0,
                },
            ],
            hotLines: [
                {
                    file: '/src/math.py',
                    line: 12,
                    samples: 2000,
                    percentage: 40.0,
                },
            ],
        };

        assert.strictEqual(result.hotFunctions.length, 2, 'Should have 2 hot functions');
        assert.strictEqual(result.hotLines.length, 1, 'Should have 1 hot line');
        assert.strictEqual(result.hotFunctions[0].name, 'compute');
        assert.strictEqual(result.hotFunctions[1].name, 'transform');
        assert.ok(result.hotFunctions[0].percentage > result.hotFunctions[1].percentage,
            'First function should have higher percentage');
        assert.ok(result.hotFunctions[0].selfPercentage <= result.hotFunctions[0].percentage,
            'selfPercentage should not exceed percentage');
    });
});
