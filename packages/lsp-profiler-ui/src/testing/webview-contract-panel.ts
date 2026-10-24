// Implements [PROF-UI-WEBVIEW] (once-bound message handler). See docs/specs/LSP-PROFILER-SPEC.md
/**
 * A singleton panel binds its message handler once, however often it is
 * re-rendered, and drops a posted message that is not a profiler message.
 * A webview delivers its posts in order, so a closing marker post tells the
 * test every earlier post has been delivered or dropped: nothing waits on time.
 */

import * as assert from 'assert';
import { buildWebviewDocument, SingletonWebviewPanel, type WebviewMessage } from '../profiler-webview';
import { closeAllEditors, pollUntilResult } from './webview-contract-helpers';

/** A document whose script posts `ready` messages for `files`, in order. */
function postingDoc(files: readonly string[]): string {
    const posts = files.map((file) => `api.postMessage({ type: 'ready', file: '${file}' });`).join(' ');
    return buildWebviewDocument({ title: 'handler-once probe', css: '', body: '<p>probe</p>',
        script: `const api = globalThis.profilerVsCodeApi || (globalThis.profilerVsCodeApi = acquireVsCodeApi()); ${posts}` });
}

/** The files of the messages received, in arrival order. */
const filesOf = (received: readonly WebviewMessage[]): string[] => received.map((m) => m.file ?? '?');

async function reRenderingNeverStacksAHandler(this: Mocha.Context): Promise<void> {
    this.timeout(30_000);
    const received: WebviewMessage[] = [];
    const panel = new SingletonWebviewPanel(() => 'lspui.test.handlerOnce', (msg) => { received.push(msg); });
    try {
        panel.show('probe', postingDoc(['first']));
        await pollUntilResult(() => received.length, (count) => count >= 1);
        // With a handler re-registered per open, every post of the second
        // render is delivered twice, and `second` arrives again before `end`.
        panel.show('probe', postingDoc(['second', 'end']));
        await pollUntilResult(() => filesOf(received).includes('end'), (ended) => ended);
        assert.deepStrictEqual(filesOf(received), ['first', 'second', 'end'], 'each render must deliver its messages exactly once; a repeat means stacked handlers');
        assert.ok(panel.isOpen(), 'the singleton panel must stay open across re-renders');
    } finally {
        panel.dispose();
    }
    assert.ok(!panel.isOpen(), 'dispose must settle the panel closed');
}

async function malformedMessagesNeverReachTheHandler(this: Mocha.Context): Promise<void> {
    this.timeout(30_000);
    const received: unknown[] = [];
    const panel = new SingletonWebviewPanel(() => 'lspui.test.malformed', (msg) => { received.push(msg); });
    const malformed = `[null, 7, 'ready', [], {}, { type: 5 }, { type: 'ready', file: 5 }, { type: 'ready', line: '3' }]`;
    try {
        panel.show('probe', buildWebviewDocument({ title: 'malformed probe', css: '', body: '',
            script: `const api = acquireVsCodeApi(); ${malformed}.forEach((m) => api.postMessage(m));` +
                ` api.postMessage({ type: 'ready', file: 'last', line: 3 });` }));
        await pollUntilResult(() => received.length, (count) => count >= 1);
        assert.deepStrictEqual(received, [{ type: 'ready', file: 'last', line: 3 }]);
    } finally {
        panel.dispose();
    }
}

suite('Profiler webviews — singleton panel message handler', () => {
    teardown(async () => { await closeAllEditors(); });
    test('re-opening a panel re-renders but never stacks a second message handler', reRenderingNeverStacksAHandler);
    test('a posted message that is not a well-formed profiler message never reaches the handler', malformedMessagesNeverReachTheHandler);
});
