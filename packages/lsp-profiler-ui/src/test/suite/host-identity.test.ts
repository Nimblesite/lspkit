// Tests for [PROF-UI-IDS]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * HOST IDENTITY — everything the library registers, sends or renders carries
 * the host's identity (prefix `lspui`, brand `LspUi` in the test host) and
 * nothing names another product. The library was extracted from Basilisk; these
 * tests fail if a Basilisk (or any hard-coded) identity survives anywhere a
 * user or the server can see it.
 */

import * as assert from 'assert';
import * as vscode from 'vscode';
import { brand, cmd, configSection, ctx, debugType, lsp, lspTable, processUriScheme, viewId } from '../../ids';
import { buildFlamegraphHtml } from '../../profiler-flamegraph-html';
import { buildMemoryDashboardHtml } from '../../memory-dashboard';
import { buildRefGraphHtml } from '../../memory-ref-graph';
import { brandHeading } from '../../profiler-styles';
import { EXTENSION_ID, hostApi } from './test-helpers';

const FOREIGN = /basilisk|sharplsp/i;

function flamegraph(): string {
    return buildFlamegraphHtml({
        sessionId: 's-1', duration: 1, totalSamples: 10, outputFile: '/tmp/p.json',
        hotFunctions: [{ name: 'f', file: '/a.txt', line: 1, samples: 10, percentage: 100, selfPercentage: 100 }],
        hotLines: [],
    });
}

function dashboard(): string {
    return buildMemoryDashboardHtml({
        memorySessionId: 'm-1', snapshotId: 'snap-1', currentMemory: 1, peakMemory: 1, gcObjects: 1,
        gcCounts: [0, 0, 0], topAllocations: [], timeline: [], heapProfilePath: '',
    });
}

suite('Host identity — ids follow the installed host', () => {
    suiteSetup(async () => { await hostApi(); });

    test('client commands, context keys and the view are `<prefix>.profiler.*`', () => {
        assert.strictEqual(cmd('memoryStart'), 'lspui.profiler.memoryStart');
        assert.strictEqual(ctx('cpuBusy'), 'lspui.profiler.cpuBusy');
        assert.strictEqual(viewId(), 'lspui.profiler.processes');
    });

    test('server commands are `<prefix>.lspProfiler.<area>.<action>`', () => {
        assert.strictEqual(lsp('memory.snapshot'), 'lspui.lspProfiler.memory.snapshot');
        const table = lspTable({ start: 'cpu.start' });
        assert.strictEqual(table.start, 'lspui.lspProfiler.cpu.start');
    });

    test('brand, settings section, debug type and row URI scheme come from the host', () => {
        assert.strictEqual(brand(), 'LspUi');
        assert.strictEqual(configSection(), 'lspui');
        assert.strictEqual(debugType(), 'lspui-debug');
        assert.strictEqual(processUriScheme(), 'lspui-process');
    });

    test('every command the library registered is under the host prefix; none under another product', async () => {
        const all = await vscode.commands.getCommands(true);
        const ours = all.filter((id) => id.startsWith('lspui.profiler.'));
        assert.ok(ours.includes('lspui.profiler.memoryStart'), `memoryStart registered: ${ours.join(', ')}`);
        assert.ok(ours.includes('lspui.profiler.profileStop'), `profileStop registered: ${ours.join(', ')}`);
        const foreign = all.filter((id) => /^(basilisk|sharplsp)\./i.test(id));
        assert.deepStrictEqual(foreign, [], 'no command under another product prefix');
    });

    test('no generated command title repeats its category — VS Code already shows "<category>: <title>"', () => {
        const manifest: unknown = vscode.extensions.getExtension(EXTENSION_ID)?.packageJSON;
        const contributes: unknown = typeof manifest === 'object' && manifest !== null ? Reflect.get(manifest, 'contributes') : undefined;
        const commands: unknown = typeof contributes === 'object' && contributes !== null ? Reflect.get(contributes, 'commands') : undefined;
        assert.ok(Array.isArray(commands) && commands.length > 0, 'the manifest declares commands');
        for (const entry of commands as unknown[]) {
            const title = String(Reflect.get(entry as object, 'title'));
            const category = String(Reflect.get(entry as object, 'category'));
            assert.strictEqual(category, 'LspUi', `${title} sits under the host's category`);
            assert.ok(!title.startsWith(category), `"${title}" must not repeat "${category}"`);
        }
    });
});

suite('Host identity — webviews render the host brand', () => {
    suiteSetup(async () => { await hostApi(); });

    test('the CPU results heading is the host brand, upper-cased', () => {
        const html = flamegraph();
        assert.ok(html.includes('<span class="accent">LSPUI</span> PROFILER'), 'brand heading present');
        assert.ok(!FOREIGN.test(html), 'no other product named in the CPU results');
    });

    test('the memory dashboard heading is the host brand, upper-cased', () => {
        const html = dashboard();
        assert.ok(html.includes('<span class="accent">LSPUI</span> MEMORY'), 'brand heading present');
        assert.ok(!FOREIGN.test(html), 'no other product named in the dashboard');
    });

    test('the retention graph names no other product', () => {
        const html = buildRefGraphHtml({ targetType: 'Leak', maxDepth: 5, maxNodes: 50, script: '', graph: { nodes: [], edges: [], cycles: [] } });
        assert.ok(!FOREIGN.test(html), 'no other product named in the retention graph');
    });

    test('a brand carrying markup is escaped, never injected into the heading', () => {
        const heading = brandHeading('<img src=x>', 'PROFILER');
        assert.ok(!heading.includes('<img'), `markup escaped: ${heading}`);
        assert.ok(heading.includes('&lt;IMG SRC=X&gt;'), `escaped brand present: ${heading}`);
    });
});
