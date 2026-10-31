// Implements [PROF-UI-PALETTE]. See docs/specs/LSP-PROFILER-SPEC.md
import * as assert from 'node:assert';
import * as fs from 'node:fs';
import * as os from 'node:os';
import path from 'node:path';
import * as vscode from 'vscode';
import type { LeakConfidence } from '../memory-decorations';
import { appliedMemoryDecorations, applyLeakDecorations, clearMemoryDecorations } from '../memory-decorations';
import { profilerPalette } from '../palette';
import { closeAllEditors, removeTestDir } from './webview-contract-helpers';
import { editorPathKey } from '../editor-path-key';

const CONFIDENCES: readonly LeakConfidence[] = ['DEFINITE', 'HIGH', 'MEDIUM', 'LOW'];

/** Paint one leak of each confidence on an open file and return the colours the editor got. */
async function leakColours(): Promise<string[]> {
    const dir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'lspui-leak-palette-'));
    const file = path.join(dir, 'leaks.txt');
    await fs.promises.writeFile(file, 'definite\nhigh\nmedium\nlow\n');
    try {
        await vscode.window.showTextDocument(vscode.Uri.file(file));
        applyLeakDecorations({ totalGrowth: 0, totalFreed: 0, netGrowth: 0, suspectedLeaks: CONFIDENCES.map((confidence, i) => ({
            file, line: i + 1, sizeGrowth: 1024, countGrowth: 1, currentSize: 2048, confidence, reason: 'grows', typeName: '' })) });
        return appliedMemoryDecorations()
            .filter((entry) => editorPathKey(entry.file) === editorPathKey(file))
            .map((entry) => entry.color);
    } finally {
        clearMemoryDecorations();
        await closeAllEditors();
        removeTestDir(dir);
    }
}

/** Every leak confidence is painted in a colour the host palette actually declares. */
export async function leakDecorationsWearTheHostPalette(): Promise<void> {
    const hostColours = Object.values(profilerPalette());
    const colours = await leakColours();
    assert.strictEqual(colours.length, CONFIDENCES.length, 'every leak is painted');
    for (const colour of colours) {
        assert.ok(hostColours.includes(colour), `a leak decoration draws ${colour}, which the host palette does not have`);
    }
}
