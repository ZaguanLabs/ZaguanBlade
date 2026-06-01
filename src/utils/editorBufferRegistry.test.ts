import assert from 'node:assert/strict';
import test from 'node:test';
import {
    applyEditorContentSnapshot,
    getDirtyEditorSaveCandidates,
    normalizeEditorPath,
    pruneEditorBufferRegistry,
    type EditorBufferRegistry,
} from './editorBufferRegistry';

test('normalizeEditorPath canonicalizes separators and trailing slash', () => {
    assert.equal(normalizeEditorPath('foo\\bar//baz/'), 'foo/bar/baz');
});

test('applyEditorContentSnapshot stores dirty drafts by normalized path', () => {
    const registry = applyEditorContentSnapshot({}, {
        filePath: 'src\\file.ts',
        savedContent: 'base',
        draftContent: 'draft',
        isDirty: true,
    });

    assert.deepEqual(registry['src/file.ts'], {
        path: 'src/file.ts',
        cleanContent: 'base',
        draftContent: 'draft',
        dirty: true,
        version: 1,
    });
});

test('applyEditorContentSnapshot clears draft when file becomes clean', () => {
    const dirty = applyEditorContentSnapshot({}, {
        filePath: 'src/file.ts',
        savedContent: 'base',
        draftContent: 'draft',
        isDirty: true,
    });
    const clean = applyEditorContentSnapshot(dirty, {
        filePath: 'src/file.ts',
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
    });

    assert.deepEqual(clean['src/file.ts'], {
        path: 'src/file.ts',
        cleanContent: 'saved',
        draftContent: undefined,
        dirty: false,
        version: 2,
    });
});

test('getDirtyEditorSaveCandidates only emits dirty buffers with explicit drafts', () => {
    const registry: EditorBufferRegistry = {
        'src/a.ts': {
            path: 'src/a.ts',
            cleanContent: 'a-base',
            draftContent: 'a-draft',
            dirty: true,
            version: 1,
        },
        'src/b.ts': {
            path: 'src/b.ts',
            cleanContent: 'b-base',
            dirty: true,
            version: 1,
        },
        'src/c.ts': {
            path: 'src/c.ts',
            cleanContent: 'c',
            dirty: false,
            version: 1,
        },
    };

    assert.deepEqual(getDirtyEditorSaveCandidates(registry), [
        {
            path: 'src/a.ts',
            baseline: 'a-base',
            draft: 'a-draft',
            source: 'editor-buffer-registry',
        },
    ]);
});

test('pruneEditorBufferRegistry drops buffers for closed paths', () => {
    const registry: EditorBufferRegistry = {
        'src/a.ts': {
            path: 'src/a.ts',
            cleanContent: 'a-base',
            draftContent: 'a-draft',
            dirty: true,
            version: 1,
        },
        'src/b.ts': {
            path: 'src/b.ts',
            cleanContent: 'b-base',
            draftContent: 'b-draft',
            dirty: true,
            version: 1,
        },
    };

    assert.deepEqual(pruneEditorBufferRegistry(registry, ['src\\a.ts']), {
        'src/a.ts': registry['src/a.ts'],
    });
});
