// Implements [PROF-EXPORT-FLAMEGRAPH] + [PROF-UI-DEBUG-LINK]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The CPU results panel leads with the exported flame graph, offers the raw
 * trace on demand, degrades to its tables without one, and keeps a debugger
 * selection from another session, or a hostile one, out of it.
 */

import * as assert from 'node:assert';
import * as fs from 'node:fs';
import * as os from 'node:os';
import path from 'node:path';
import { profileVariable, selectedProfileVariable } from '../debug-selection';
import { buildFlamegraphHtml, loadFlamegraphSvgDataUri } from '../profiler-flamegraph-html';
import type { ProfileResult } from '../profiler-decorations';
import { HOSTILE, assertHardened, profileResult, removeTestDir } from './webview-contract-helpers';

/** A result with one hot function and one hot line, overriding what a test cares about. */
function hotProfileResult(overrides: Partial<ProfileResult> = {}): ProfileResult {
    return profileResult({
        hotFunctions: [{ name: 'hot', file: '/app/main.py', line: 3, samples: 200, percentage: 80, selfPercentage: 75 }],
        hotLines: [{ file: '/app/main.py', line: 5, samples: 180, percentage: 72 }],
        ...overrides,
    });
}

let tmpDir = '';

async function writeSvg(name: string, contents: string): Promise<string> {
    const svgPath = path.join(tmpDir, name);
    await fs.promises.writeFile(svgPath, contents);
    return svgPath;
}

async function theFlameGraphIsTheHero(): Promise<void> {
    const svgPath = await writeSvg('profile.svg', '<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>');
    const html = await buildFlamegraphHtml(hotProfileResult({ flamegraphPath: svgPath }));
    assert.ok(html.includes('data:image/svg+xml;base64,'), 'the flame graph SVG must be inlined as a data URI (a profiler must show a flame graph)');
    assert.ok(html.includes('open-flame-svg'), 'the hero must offer opening the interactive SVG externally');
    assert.ok(html.includes('openFlamegraphSvg'), 'the open action must post a message the extension handles');
}

// [PROF-CPU-START] The built-in `.cpuprofile` viewer is on-demand, not the
// landing view — so the panel itself must carry the way into it. Without
// this button the raw trace is only reachable through the (dismissable)
// completion toast.
async function theRawTraceIsOneClickAway(): Promise<void> {
    const withTrace = await buildFlamegraphHtml(hotProfileResult({ cpuProfilePath: '/tmp/lspui-s-1.cpuprofile' }));
    assert.ok(withTrace.includes('Open Trace in VS Code Viewer'), 'the panel must offer opening the native .cpuprofile viewer');
    assert.ok(withTrace.includes('openCpuProfile'), 'the trace button must post a message the extension handles');
    const withoutTrace = await buildFlamegraphHtml(hotProfileResult());
    assert.ok(!withoutTrace.includes('/tmp/lspui-s-1.cpuprofile'), 'no stale trace path may be embedded when no .cpuprofile was produced');
}

async function noSvgDegradesToTheTables(): Promise<void> {
    const withoutPath = await buildFlamegraphHtml(hotProfileResult());
    const withDeadPath = await buildFlamegraphHtml(hotProfileResult({ flamegraphPath: path.join(tmpDir, 'nope.svg') }));
    for (const html of [withoutPath, withDeadPath]) {
        assert.ok(!html.includes('data:image/svg+xml'), 'no hero image without a readable SVG');
        assert.ok(!html.includes('<img'), 'no broken <img> element');
        assert.ok(html.includes('fn-body'), 'the hot-functions table must still render');
    }
}

/** The largest SVG the hero inlines, in MiB. */
const SVG_LIMIT_MIB = 5;
const MEBIBYTE = 1024;

async function unusableSvgsAreNotInlined(): Promise<void> {
    assert.strictEqual(await loadFlamegraphSvgDataUri(undefined), undefined);
    assert.strictEqual(await loadFlamegraphSvgDataUri(''), undefined);
    const empty = await writeSvg('empty.svg', '');
    assert.strictEqual(await loadFlamegraphSvgDataUri(empty), undefined, 'an empty artifact is not a flame graph');
    const oversized = await writeSvg('huge.svg', `<svg>${'x'.repeat(SVG_LIMIT_MIB * MEBIBYTE * MEBIBYTE)}</svg>`);
    assert.strictEqual(await loadFlamegraphSvgDataUri(oversized), undefined, 'an oversized artifact must not be inlined (it still opens externally)');
}

// Implements [PROF-EXPORT-FLAMEGRAPH]. See docs/specs/LSP-PROFILER-SPEC.md
suite('Profiler webviews — flame graph hero', () => {
    suiteSetup(async () => { tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'lspui-flame-hero-')); });
    suiteTeardown(async () => { await removeTestDir(tmpDir); });
    test('the results panel embeds the LSP-exported flame graph SVG as its hero', theFlameGraphIsTheHero);
    test('the results panel offers the raw trace via an "Open Trace in VS Code Viewer" button', theRawTraceIsOneClickAway);
    test('a missing or unreadable SVG degrades to the tables, never a broken image', noSvgDegradesToTheTables);
    test('loadFlamegraphSvgDataUri refuses empty and oversized artifacts', unusableSvgsAreNotInlined);
});

function selectionsStayInTheirSession(): void {
    const variable = { name: 'child', evaluateName: 'parent.child', value: '42', type: 'Record' };
    const selected = profileVariable({ sessionId: 'one', variable }, 'one');
    assert.equal(selected?.expression, 'parent.child');
    assert.equal(selected?.type, 'Record');
    assert.equal(selected?.value, '42');
    assert.equal(profileVariable({ sessionId: 'other', variable }, 'one'), undefined);
    assert.equal(profileVariable({ variable: { name: '', value: '' } }, 'one'), undefined);
}

async function hostileSelectionsStayInert(): Promise<void> {
    selectedProfileVariable.value = profileVariable({ name: HOSTILE, value: HOSTILE, type: HOSTILE }, 'one');
    try { assertHardened(await buildFlamegraphHtml(hotProfileResult()), 'debug variable'); }
    finally { selectedProfileVariable.value = undefined; }
}

// Implements [PROF-UI-DEBUG-LINK]. The same session boundary is tested in each VSIX.
suite('Profiler debugger selection contract', () => {
    test('preserves expressions and rejects a selection from another session', selectionsStayInTheirSession);
    test('debugger values cannot escape into executable webview content', hostileSelectionsStayInert);
});
