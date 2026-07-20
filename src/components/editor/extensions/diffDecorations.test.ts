import assert from 'node:assert/strict';
import test from 'node:test';
import { EditorState } from '@codemirror/state';
import {
    diffStateField,
    parseUnifiedDiff,
    resolveDiffStateUpdate,
    setDiffState,
} from './diffDecorations';

test('parseUnifiedDiff maps consecutive changelog insertions to both added lines', () => {
    const diff = `diff --git a/src/pages/changelog.astro b/src/pages/changelog.astro
index 08f9f61..107c2ed 100644
--- a/src/pages/changelog.astro
+++ b/src/pages/changelog.astro
@@ -42,6 +42,8 @@ import BaseLayout from '../layouts/BaseLayout.astro';
                 <li><strong>Dependencies.</strong> Updated Tauri to the latest version.</li>
                 <li><strong>AI Chat.</strong> Optimized the AI Chat based on technical inspiration from <a href="https://github.com/pingdotgg/t3code" target="_blank" rel="noopener">T3Code</a>.</li>
                 <li><strong>CodeMirror Editor.</strong> Optimized the CodeMirror editor to get React out of the way to make it faster, more reliable and better overall.</li>
+                <li><strong>PHP Support.</strong> Added highlighting for PHP in CodeMirror.</li>
+                <li><strong>Node Modules.</strong> Updated Node modules and crates for both frontend and backend.
                 <li><strong>Zaguán Coder Daemon.</strong> Enabled parallel tool calling for DeepSeek v4.</li>
                 <li><strong>Zaguán Coder Daemon.</strong> Fixed a bug that prevented Mistral 3.5 from working past the first response.</li>
                 <li><strong>Zaguán Blade.</strong> Created a new version of the AI Chat Panel.</li>
`;

    const addedLines = parseUnifiedDiff(diff).filter(line => line.type === 'added');

    assert.deepEqual(
        addedLines.map(line => line.newLineNum),
        [45, 46],
    );
    assert.equal(addedLines[0]?.content.includes('PHP Support'), true);
    assert.equal(addedLines[1]?.content.includes('Node Modules'), true);
});

test('an omitted diff update preserves the visible review diff during a document reload', () => {
    const initialDiff = `--- a/file
+++ b/file
@@ -1 +1 @@
-before
+after
`;
    let state = EditorState.create({
        doc: 'after',
        extensions: [diffStateField],
    });
    const initialDiffState = resolveDiffStateUpdate(null, initialDiff);

    state = state.update({ effects: setDiffState.of(initialDiffState) }).state;
    const visibleDiffState = state.field(diffStateField);

    state = state.update({
        changes: { from: 0, to: state.doc.length, insert: 'after again' },
        effects: setDiffState.of(resolveDiffStateUpdate(visibleDiffState, undefined)),
    }).state;

    assert.strictEqual(state.field(diffStateField), visibleDiffState);
    assert.equal(state.field(diffStateField)?.lines.some(line => line.type === 'added'), true);
});

test('an explicit null diff update clears the visible review diff', () => {
    const current = resolveDiffStateUpdate(null, `--- a/file
+++ b/file
@@ -1 +1 @@
-before
+after
`);

    assert.equal(resolveDiffStateUpdate(current, null), null);
});
