// Implements [PROF-UI-PALETTE]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Every profiler view draws its heat, memory and leak accents in the host's
 * palette: a host that supplies green never shows a purple bar, a label that
 * names a colour it does not draw, or dark text on a dark accent.
 */

import * as assert from 'assert';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import { configSection } from '../ids';
import { buildMemoryDashboardHtml, type MemoryDiffData } from '../memory-dashboard';
import { buildRefGraphHtml } from '../memory-ref-graph';
import { applyLeakDecorations, appliedMemoryDecorations, clearMemoryDecorations, type LeakConfidence } from '../memory-decorations';
import { DEFAULT_PALETTE, inkFor, paletteCss, profilerPalette, type ProfilerPalette } from '../palette';
import { FLAME_SCRIPT } from '../flame-script';
import { buildFlamegraphHtml } from '../profiler-flamegraph-html';
import { closeAllEditors, dashboardSnapshot, profileResult, removeTestDir } from './webview-contract-helpers';

/** A dark host palette that shares no colour with the default one. */
const DARK_PALETTE: Required<ProfilerPalette> = {
    critical: '#1b4d2e', hot: '#235c37', warm: '#2e6b41', memory: '#14324a', memoryHot: '#1c4060',
    memoryWarm: '#244d72', memoryCool: '#2c5a84', leak: '#5a1c1c', success: '#1c5a3c',
};

const LEAKING_DIFF: MemoryDiffData = {
    totalGrowth: 2048, totalFreed: 0, netGrowth: 2048, grownAllocations: [],
    suspectedLeaks: (['definite', 'high', 'medium', 'low'] as const).map((confidence) => ({
        file: '/app/leak.py', line: 4, sizeGrowth: 2048, countGrowth: 2,
        currentSize: 4096, currentCount: 4, confidence, reason: 'grows on every pass',
    })),
};

/** Every profiler view, rendered under whatever palette is configured now. */
function everyView(): Record<string, string> {
    return {
        'memory dashboard': buildMemoryDashboardHtml(dashboardSnapshot(), LEAKING_DIFF),
        'retention graph': buildRefGraphHtml({ targetType: 'W', maxDepth: 5, maxNodes: 200, script: '' }),
        'results panel': buildFlamegraphHtml(profileResult()),
    };
}

/** Render every view with `palette` as the user's override, then restore the setting. */
async function viewsUnder(palette: ProfilerPalette): Promise<Record<string, string>> {
    const config = vscode.workspace.getConfiguration(configSection());
    const previous = config.inspect('profiler.palette')?.globalValue;
    await config.update('profiler.palette', palette, vscode.ConfigurationTarget.Global);
    try {
        return everyView();
    } finally {
        await config.update('profiler.palette', previous, vscode.ConfigurationTarget.Global);
    }
}

async function noViewKeepsADefaultAccent(): Promise<void> {
    for (const [view, html] of Object.entries(await viewsUnder(DARK_PALETTE))) {
        assert.ok(html.includes(paletteCss(DARK_PALETTE)), `the ${view} declares the host palette`);
        for (const [role, hex] of Object.entries(DEFAULT_PALETTE)) {
            assert.ok(!html.toLowerCase().includes(String(hex)), `the ${view} draws the default ${role} ${String(hex)} under a host palette`);
        }
    }
}

function inkContrastsWithItsFill(): void {
    assert.equal(inkFor(DARK_PALETTE.hot), inkFor('#000000'), 'a dark accent carries light ink');
    assert.equal(inkFor(DEFAULT_PALETTE.warm), inkFor('#ffffff'), 'a light accent carries dark ink');
    assert.notEqual(inkFor('#000000'), inkFor('#ffffff'));
    assert.ok(paletteCss(DARK_PALETTE).includes(`--prof-hot-ink: ${inkFor(DARK_PALETTE.hot)};`), 'the hot accent declares its ink');
}

async function textOnAnAccentReadsItsInk(): Promise<void> {
    const results = (await viewsUnder(DARK_PALETTE))['results panel'] ?? '';
    assert.ok(/\.toggle\.active \{[^}]*color: var\(--prof-hot-ink\)/.test(results), 'the active toggle writes in the hot accent\'s ink');
    assert.ok(FLAME_SCRIPT.includes('ctx.fillStyle = inkFor(model.frames[r.frameIdx].color)'), 'a slab label is inked against its own slab');
    assert.ok(FLAME_SCRIPT.includes(inkFor.toString()), 'the canvas runs the tested ink rule');
}

function labelsNameNoColour(): void {
    const dashboard = buildMemoryDashboardHtml(dashboardSnapshot());
    assert.ok(dashboard.includes('id="btn-mem">Memory</button>'), 'the memory heat toggle names its metric, not a colour');
    assert.ok(dashboard.includes('id="btn-cpu">CPU</button>'), 'the CPU heat toggle names its metric, not a colour');
}

const CONFIDENCES: readonly LeakConfidence[] = ['DEFINITE', 'HIGH', 'MEDIUM', 'LOW'];

/** Paint one leak of each confidence on an open file and return the colours the editor got. */
async function leakColours(): Promise<string[]> {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'lspui-leak-palette-'));
    const file = path.join(dir, 'leaks.txt');
    fs.writeFileSync(file, 'definite\nhigh\nmedium\nlow\n');
    try {
        await vscode.window.showTextDocument(vscode.Uri.file(file));
        applyLeakDecorations({ totalGrowth: 0, totalFreed: 0, netGrowth: 0, suspectedLeaks: CONFIDENCES.map((confidence, i) => ({
            file, line: i + 1, sizeGrowth: 1024, countGrowth: 1, currentSize: 2048, confidence, reason: 'grows' })) });
        return appliedMemoryDecorations().filter((entry) => entry.file === file).map((entry) => entry.color);
    } finally {
        clearMemoryDecorations();
        await closeAllEditors();
        removeTestDir(dir);
    }
}

async function leakDecorationsWearTheHostPalette(): Promise<void> {
    const hostColours = Object.values(profilerPalette());
    const colours = await leakColours();
    assert.strictEqual(colours.length, CONFIDENCES.length, 'every leak is painted');
    for (const colour of colours) {
        assert.ok(hostColours.includes(colour), `a leak decoration draws ${colour}, which the host palette does not have`);
    }
}

suite('Profiler webviews — host palette', () => {
    test('no view draws a default accent under a host palette', noViewKeepsADefaultAccent);
    test('ink contrasts with the accent it is written on', inkContrastsWithItsFill);
    test('text on an accent is written in that accent\'s ink', textOnAnAccentReadsItsInk);
    test('a heat toggle names its metric, never a colour', labelsNameNoColour);
    test('editor leak decorations wear the host palette at every confidence', leakDecorationsWearTheHostPalette);
});
