import assert from 'node:assert/strict';
import test from 'node:test';
import {
    applyEditorContentSnapshot,
    applyEditorContentSnapshots,
    getDirtyEditorSaveCandidates,
    getEditorContentSnapshotForPath,
    getOpenDirtyEditorSaveCandidates,
    markEditorSaveCandidatesClean,
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

test('applyEditorContentSnapshot preserves existing dirty draft when next dirty snapshot omits draft', () => {
    const dirty = applyEditorContentSnapshot({}, {
        filePath: 'src/file.ts',
        savedContent: 'base',
        draftContent: 'draft',
        isDirty: true,
    });
    const metadataOnly = applyEditorContentSnapshot(dirty, {
        filePath: 'src/file.ts',
        savedContent: 'base',
        draftContent: undefined,
        isDirty: true,
    });

    assert.deepEqual(metadataOnly['src/file.ts'], {
        path: 'src/file.ts',
        cleanContent: 'base',
        draftContent: 'draft',
        dirty: true,
        version: 1,
    });
    assert.equal(metadataOnly, dirty);
});

test('applyEditorContentSnapshots applies snapshots in order', () => {
    const registry = applyEditorContentSnapshots({}, [
        {
            filePath: 'src/a.ts',
            savedContent: 'a-base',
            draftContent: 'a-draft',
            isDirty: true,
        },
        {
            filePath: 'src/b.ts',
            savedContent: 'b-base',
            draftContent: 'b-draft',
            isDirty: true,
        },
        {
            filePath: 'src/a.ts',
            savedContent: 'a-saved',
            draftContent: undefined,
            isDirty: false,
        },
    ]);

    assert.deepEqual(registry, {
        'src/a.ts': {
            path: 'src/a.ts',
            cleanContent: 'a-saved',
            draftContent: undefined,
            dirty: false,
            version: 2,
        },
        'src/b.ts': {
            path: 'src/b.ts',
            cleanContent: 'b-base',
            draftContent: 'b-draft',
            dirty: true,
            version: 1,
        },
    });
});

test('getEditorContentSnapshotForPath prefers registry over fallback', () => {
    const registry = applyEditorContentSnapshot({}, {
        filePath: 'src/file.ts',
        savedContent: 'base',
        draftContent: 'draft',
        isDirty: true,
    });

    assert.deepEqual(getEditorContentSnapshotForPath(registry, 'src\\file.ts', {
        savedContent: 'fallback-base',
        draftContent: 'fallback-draft',
        isDirty: false,
    }), {
        filePath: 'src\\file.ts',
        savedContent: 'base',
        draftContent: 'draft',
        isDirty: true,
    });
});

test('getEditorContentSnapshotForPath does not merge stale fallback fields into registry buffer', () => {
    const registry = applyEditorContentSnapshot({}, {
        filePath: 'src/file.ts',
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
    });

    assert.deepEqual(getEditorContentSnapshotForPath(registry, 'src/file.ts', {
        savedContent: 'stale-base',
        draftContent: 'stale-draft',
        isDirty: true,
    }), {
        filePath: 'src/file.ts',
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
    });
});

test('getEditorContentSnapshotForPath falls back when registry has no path', () => {
    assert.deepEqual(getEditorContentSnapshotForPath({}, 'src/file.ts', {
        savedContent: 'fallback-base',
        draftContent: 'fallback-draft',
        isDirty: true,
    }), {
        filePath: 'src/file.ts',
        savedContent: 'fallback-base',
        draftContent: 'fallback-draft',
        isDirty: true,
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

test('getOpenDirtyEditorSaveCandidates filters dirty buffers to open paths', () => {
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

    assert.deepEqual(getOpenDirtyEditorSaveCandidates(registry, ['src\\b.ts']), [
        {
            path: 'src/b.ts',
            baseline: 'b-base',
            draft: 'b-draft',
            source: 'editor-buffer-registry',
        },
    ]);
});

test('clean save snapshot removes open dirty save candidate', () => {
    const dirty = applyEditorContentSnapshot({}, {
        filePath: 'src/file.ts',
        savedContent: 'base',
        draftContent: 'draft',
        isDirty: true,
    });
    const saved = applyEditorContentSnapshot(dirty, {
        filePath: 'src/file.ts',
        savedContent: 'draft',
        draftContent: undefined,
        isDirty: false,
    });

    assert.deepEqual(getOpenDirtyEditorSaveCandidates(saved, ['src/file.ts']), []);
    assert.deepEqual(saved['src/file.ts'], {
        path: 'src/file.ts',
        cleanContent: 'draft',
        draftContent: undefined,
        dirty: false,
        version: 2,
    });
});

test('markEditorSaveCandidatesClean advances saved candidates to clean baselines', () => {
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
    const candidates = getOpenDirtyEditorSaveCandidates(registry, ['src/a.ts', 'src/b.ts']);
    const saved = markEditorSaveCandidatesClean(registry, candidates);

    assert.deepEqual(getOpenDirtyEditorSaveCandidates(saved, ['src/a.ts', 'src/b.ts']), []);
    assert.deepEqual(saved['src/a.ts'], {
        path: 'src/a.ts',
        cleanContent: 'a-draft',
        draftContent: undefined,
        dirty: false,
        version: 2,
    });
    assert.deepEqual(saved['src/b.ts'], {
        path: 'src/b.ts',
        cleanContent: 'b-draft',
        draftContent: undefined,
        dirty: false,
        version: 2,
    });
});

test('markEditorSaveCandidatesClean normalizes saved candidate paths', () => {
    const registry: EditorBufferRegistry = {
        'src/file.ts': {
            path: 'src/file.ts',
            cleanContent: 'base',
            draftContent: 'draft',
            dirty: true,
            version: 1,
        },
    };
    const saved = markEditorSaveCandidatesClean(registry, [{
        path: 'src\\file.ts',
        baseline: 'base',
        draft: 'draft',
        source: 'editor-buffer-registry',
    }]);

    assert.deepEqual(saved['src/file.ts'], {
        path: 'src/file.ts',
        cleanContent: 'draft',
        draftContent: undefined,
        dirty: false,
        version: 2,
    });
    assert.deepEqual(getDirtyEditorSaveCandidates(saved), []);
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
