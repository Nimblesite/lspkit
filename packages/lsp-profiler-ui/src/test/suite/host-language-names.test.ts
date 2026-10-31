// Tests for [PROF-UI-HOST]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * A MESSAGE NAMES THE HOST'S LANGUAGES AS A USER DOES. "Run & profile" with no
 * file of the host's languages open told a SharpLsp user to "Open a
 * csharp/fsharp file": VS Code's language IDs, not language names.
 */

import * as assert from 'node:assert';
import { openFileToProfile } from '../../process-launch';

suite('[PROF-UI-HOST] messages name the host languages as a user knows them', () => {
    test('two languages are named, joined in words, in the order the host gave', () => {
        const said = openFileToProfile(['F#', 'C#']);
        assert.ok(said.endsWith('Open a file in F# or C# to run and profile.'), said);
        assert.ok(!said.includes('/'), said);
    });

    test('one language is named alone, and no editor language id leaks through', () => {
        const said = openFileToProfile(['Plain Text']);
        assert.ok(said.endsWith('Open a file in Plain Text to run and profile.'), said);
        assert.ok(!said.includes('plaintext'), said);
    });
});
