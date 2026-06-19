import assert from 'node:assert/strict';
import test from 'node:test';
import {
    applyEditorContentSnapshot,
    applyEditorContentSnapshotToMirror,
    applyEditorContentSnapshots,
    createEditorContentSnapshot,
    getEditorContentStatePropagation,
    getDirtyEditorSaveCandidates,
    getDirtyEditorSaveCandidateDisplayNames,
    getEditorContentSnapshotForMirror,
    getEditorContentSnapshotForPath,
    getEditorContentSnapshotFromMirror,
    getEditorContentSnapshotsFromMirrors,
    getEditorContentSnapshotTargetId,
    getEditorInitialContentConfig,
    getOpenDirtyEditorSaveCandidates,
    getOpenDirtyEditorSaveCandidatesFromMirrors,
    getTabDirtyStates,
    markEditorSaveCandidatesClean,
    normalizeEditorPath,
    pruneEditorBufferRegistry,
    shouldFlushEditorContentSnapshotImmediately,
    type EditorBufferRegistry,
} from './editorBufferRegistry';

test('normalizeEditorPath canonicalizes separators and trailing slash', () => {
    assert.equal(normalizeEditorPath('foo\\bar//baz/'), 'foo/bar/baz');
});

test('createEditorContentSnapshot derives clean state from matching current and baseline content', () => {
    assert.deepEqual(createEditorContentSnapshot({
        filePath: 'src/file.ts',
        baselineContent: 'same',
        currentContent: 'same',
    }), {
        filePath: 'src/file.ts',
        savedContent: 'same',
        draftContent: undefined,
        isDirty: false,
    });
});

test('createEditorContentSnapshot derives dirty state from changed current content', () => {
    assert.deepEqual(createEditorContentSnapshot({
        filePath: 'src/file.ts',
        baselineContent: 'baseline',
        currentContent: 'draft',
    }), {
        filePath: 'src/file.ts',
        savedContent: 'baseline',
        draftContent: 'draft',
        isDirty: true,
    });
});

test('createEditorContentSnapshot preserves deleted-on-disk metadata when present', () => {
    assert.deepEqual(createEditorContentSnapshot({
        filePath: 'src/file.ts',
        baselineContent: 'baseline',
        currentContent: 'draft',
        isDeletedOnDisk: true,
    }), {
        filePath: 'src/file.ts',
        savedContent: 'baseline',
        draftContent: 'draft',
        isDirty: true,
        isDeletedOnDisk: true,
    });
});

test('getEditorContentStatePropagation flushes clean snapshots immediately', () => {
    const clean = createEditorContentSnapshot({
        filePath: 'src/file.ts',
        baselineContent: 'same',
        currentContent: 'same',
    });

    assert.deepEqual(getEditorContentStatePropagation(clean, true), {
        immediate: clean,
    });
});

test('getEditorContentStatePropagation emits metadata-only first dirty transition before debounced draft', () => {
    const dirty = createEditorContentSnapshot({
        filePath: 'src/file.ts',
        baselineContent: 'baseline',
        currentContent: 'draft',
    });

    assert.deepEqual(getEditorContentStatePropagation(dirty, false), {
        immediate: {
            filePath: 'src/file.ts',
            savedContent: 'baseline',
            draftContent: undefined,
            isDirty: true,
        },
        debounced: dirty,
    });
});

test('getEditorContentStatePropagation debounces subsequent dirty drafts', () => {
    const dirty = createEditorContentSnapshot({
        filePath: 'src/file.ts',
        baselineContent: 'baseline',
        currentContent: 'draft',
    });

    assert.deepEqual(getEditorContentStatePropagation(dirty, true), {
        debounced: dirty,
    });
});

test('applyEditorContentSnapshotToMirror mirrors clean snapshots and clears draft content', () => {
    const mirror = {
        savedContent: 'old saved',
        draftContent: 'old draft',
        isDirty: true,
        extra: 'preserved',
    };

    assert.deepEqual(applyEditorContentSnapshotToMirror(mirror, {
        filePath: 'src/file.ts',
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
    }), {
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
        extra: 'preserved',
    });
});

test('applyEditorContentSnapshotToMirror can avoid mirroring dirty draft and saved content', () => {
    const mirror = {
        savedContent: 'existing saved',
        draftContent: 'existing draft',
        isDirty: false,
        extra: 'preserved',
    };

    assert.deepEqual(applyEditorContentSnapshotToMirror(mirror, {
        filePath: 'src/file.ts',
        savedContent: 'next saved',
        draftContent: 'next draft',
        isDirty: true,
    }, {
        mirrorDraftContent: false,
        mirrorDirtySavedContent: false,
    }), {
        savedContent: 'existing saved',
        draftContent: undefined,
        isDirty: true,
        extra: 'preserved',
    });
});

test('applyEditorContentSnapshotToMirror returns same mirror when no mirrored state changes', () => {
    const mirror = {
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
    };

    assert.equal(applyEditorContentSnapshotToMirror(mirror, {
        filePath: 'src/file.ts',
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
    }), mirror);
});

test('applyEditorContentSnapshotToMirror sets and clears deleted-on-disk state', () => {
    const deleted = applyEditorContentSnapshotToMirror({
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
    }, {
        filePath: 'src/file.ts',
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
        isDeletedOnDisk: true,
    });

    assert.deepEqual(deleted, {
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
        isDeletedOnDisk: true,
    });

    assert.deepEqual(applyEditorContentSnapshotToMirror(deleted, {
        filePath: 'src/file.ts',
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
    }), {
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
    });
});

test('getEditorContentSnapshotFromMirror projects dirty mirror content when registry has no buffer', () => {
    assert.deepEqual(getEditorContentSnapshotFromMirror({
        path: 'src/file.ts',
        savedContent: 'saved',
        draftContent: 'draft',
        isDirty: true,
    }, false), {
        filePath: 'src/file.ts',
        savedContent: 'saved',
        draftContent: 'draft',
        isDirty: true,
    });
});

test('getEditorContentSnapshotFromMirror omits mirrored content when registry has the buffer', () => {
    assert.deepEqual(getEditorContentSnapshotFromMirror({
        path: 'src/file.ts',
        savedContent: 'stale saved',
        draftContent: 'stale draft',
        isDirty: true,
    }, true), {
        filePath: 'src/file.ts',
        savedContent: undefined,
        draftContent: undefined,
        isDirty: true,
    });
});

test('getEditorContentSnapshotFromMirror ignores mirrors without path or content state', () => {
    assert.equal(getEditorContentSnapshotFromMirror({
        savedContent: 'saved',
        isDirty: false,
    }, false), null);
    assert.equal(getEditorContentSnapshotFromMirror({
        path: 'src/file.ts',
        isDirty: false,
    }, false), null);
});

test('getEditorContentSnapshotsFromMirrors projects mirrors except excluded mirror', () => {
    assert.deepEqual(getEditorContentSnapshotsFromMirrors({}, [
        {
            id: 'active',
            path: 'src/active.ts',
            savedContent: 'active saved',
            draftContent: 'active draft',
            isDirty: true,
        },
        {
            id: 'inactive',
            path: 'src/inactive.ts',
            savedContent: 'inactive saved',
            draftContent: 'inactive draft',
            isDirty: true,
        },
        {
            id: 'clean-empty',
            path: 'src/clean-empty.ts',
            isDirty: false,
        },
    ], 'active'), [{
        filePath: 'src/inactive.ts',
        savedContent: 'inactive saved',
        draftContent: 'inactive draft',
        isDirty: true,
    }]);
});

test('getEditorContentSnapshotsFromMirrors omits mirrored content when registry has buffer', () => {
    const registry: EditorBufferRegistry = {
        'src/file.ts': {
            path: 'src/file.ts',
            cleanContent: 'registry saved',
            draftContent: 'registry draft',
            dirty: true,
            version: 1,
        },
    };

    assert.deepEqual(getEditorContentSnapshotsFromMirrors(registry, [{
        id: 'tab-1',
        path: 'src/file.ts',
        savedContent: 'mirror saved',
        draftContent: 'mirror draft',
        isDirty: true,
    }]), [{
        filePath: 'src/file.ts',
        savedContent: undefined,
        draftContent: undefined,
        isDirty: true,
    }]);
});

test('getEditorContentSnapshotTargetId falls back when snapshot has no file path', () => {
    assert.equal(getEditorContentSnapshotTargetId([
        { id: 'tab-1', path: 'src/file.ts' },
    ], 'active-tab', {
        savedContent: 'content',
        isDirty: false,
    }), 'active-tab');
});

test('getEditorContentSnapshotTargetId resolves normalized matching mirror path', () => {
    assert.equal(getEditorContentSnapshotTargetId([
        { id: 'tab-1', path: 'src/other.ts' },
        { id: 'tab-2', path: 'src\\file.ts' },
    ], 'active-tab', {
        filePath: 'src//file.ts',
        savedContent: 'content',
        isDirty: false,
    }), 'tab-2');
});

test('getEditorContentSnapshotTargetId returns null when no path matches', () => {
    assert.equal(getEditorContentSnapshotTargetId([
        { id: 'tab-1', path: 'src/other.ts' },
    ], 'active-tab', {
        filePath: 'src/file.ts',
        savedContent: 'content',
        isDirty: false,
    }), null);
});

test('shouldFlushEditorContentSnapshotImmediately flushes clean snapshots immediately', () => {
    assert.equal(shouldFlushEditorContentSnapshotImmediately({
        filePath: 'src/file.ts',
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: false,
    }), true);
});

test('shouldFlushEditorContentSnapshotImmediately flushes metadata-only dirty snapshots immediately', () => {
    assert.equal(shouldFlushEditorContentSnapshotImmediately({
        filePath: 'src/file.ts',
        savedContent: 'saved',
        draftContent: undefined,
        isDirty: true,
    }), true);
});

test('shouldFlushEditorContentSnapshotImmediately debounces dirty snapshots with draft content', () => {
    assert.equal(shouldFlushEditorContentSnapshotImmediately({
        filePath: 'src/file.ts',
        savedContent: 'saved',
        draftContent: 'draft',
        isDirty: true,
    }), false);
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

test('getEditorContentSnapshotForMirror uses mirror fields as fallback', () => {
    assert.deepEqual(getEditorContentSnapshotForMirror({}, {
        path: 'src/file.ts',
        savedContent: 'mirror saved',
        draftContent: 'mirror draft',
        isDirty: true,
    }), {
        filePath: 'src/file.ts',
        savedContent: 'mirror saved',
        draftContent: 'mirror draft',
        isDirty: true,
    });
});

test('getEditorContentSnapshotForMirror prefers registry buffer over mirror fields', () => {
    const registry: EditorBufferRegistry = {
        'src/file.ts': {
            path: 'src/file.ts',
            cleanContent: 'registry saved',
            draftContent: 'registry draft',
            dirty: true,
            version: 1,
        },
    };

    assert.deepEqual(getEditorContentSnapshotForMirror(registry, {
        path: 'src/file.ts',
        savedContent: 'mirror saved',
        draftContent: 'mirror draft',
        isDirty: false,
    }), {
        filePath: 'src/file.ts',
        savedContent: 'registry saved',
        draftContent: 'registry draft',
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

test('getOpenDirtyEditorSaveCandidates normalizes open paths', () => {
    const registry: EditorBufferRegistry = {
        'src/file.ts': {
            path: 'src/file.ts',
            cleanContent: 'base',
            draftContent: 'draft',
            dirty: true,
            version: 1,
        },
    };

    assert.deepEqual(getOpenDirtyEditorSaveCandidates(registry, ['src\\file.ts']), [
        {
            path: 'src/file.ts',
            baseline: 'base',
            draft: 'draft',
            source: 'editor-buffer-registry',
        },
    ]);
});

test('getOpenDirtyEditorSaveCandidatesFromMirrors filters dirty buffers to mirror paths', () => {
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

    assert.deepEqual(getOpenDirtyEditorSaveCandidatesFromMirrors(registry, [
        { path: 'src\\a.ts' },
        {},
    ]), [
        {
            path: 'src/a.ts',
            baseline: 'a-base',
            draft: 'a-draft',
            source: 'editor-buffer-registry',
        },
    ]);
});

test('getDirtyEditorSaveCandidateDisplayNames uses mirror titles by normalized path', () => {
    assert.deepEqual(getDirtyEditorSaveCandidateDisplayNames([
        {
            path: 'src/file.ts',
            baseline: 'base',
            draft: 'draft',
            source: 'editor-buffer-registry',
        },
    ], [
        {
            path: 'src\\file.ts',
            title: 'File title',
        },
    ], 'Untitled'), ['File title']);
});

test('getDirtyEditorSaveCandidateDisplayNames falls back to candidate path and fallback name', () => {
    assert.deepEqual(getDirtyEditorSaveCandidateDisplayNames([
        {
            path: 'src/file.ts',
            baseline: 'base',
            draft: 'draft',
            source: 'editor-buffer-registry',
        },
        {
            path: '',
            baseline: 'base',
            draft: 'draft',
            source: 'editor-buffer-registry',
        },
    ], [], 'Untitled'), ['src/file.ts', 'Untitled']);
});

test('getTabDirtyStates derives dirty state from registry buffers', () => {
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
            draftContent: undefined,
            dirty: false,
            version: 1,
        },
    };

    const dirtyStates = getTabDirtyStates(registry, [
        { id: 'tab-1', path: 'src/a.ts' },
        { id: 'tab-2', path: 'src/b.ts' },
        { id: 'tab-3', path: 'src\\c.ts' },
        { id: 'tab-4' },
    ]);

    assert.equal(dirtyStates.get('tab-1'), true);
    assert.equal(dirtyStates.get('tab-2'), false);
    assert.equal(dirtyStates.get('tab-3'), false);
    assert.equal(dirtyStates.get('tab-4'), false);
});

test('getEditorInitialContentConfig returns empty config when no active file', () => {
    assert.deepEqual(getEditorInitialContentConfig({
        activeFile: null,
        savedContent: 'saved',
        draftContent: 'draft',
        isDirty: true,
        isMarkdownFile: false,
        isFileSwitch: false,
    }), {
        content: '',
        baselineContent: '',
        shouldLoad: false,
        resetHistory: false,
        reason: null,
        awaitingInitialSync: false,
    });
});

test('getEditorInitialContentConfig loads clean non-markdown file on file switch', () => {
    assert.deepEqual(getEditorInitialContentConfig({
        activeFile: 'src/file.ts',
        savedContent: 'saved',
        draftContent: null,
        isDirty: false,
        isMarkdownFile: false,
        isFileSwitch: true,
    }), {
        content: 'saved',
        baselineContent: 'saved',
        shouldLoad: true,
        resetHistory: true,
        reason: 'open',
        awaitingInitialSync: false,
    });
});

test('getEditorInitialContentConfig loads clean non-markdown file on external update', () => {
    assert.deepEqual(getEditorInitialContentConfig({
        activeFile: 'src/file.ts',
        savedContent: 'saved',
        draftContent: null,
        isDirty: false,
        isMarkdownFile: false,
        isFileSwitch: false,
    }), {
        content: 'saved',
        baselineContent: 'saved',
        shouldLoad: false,
        resetHistory: false,
        reason: null,
        awaitingInitialSync: false,
    });
});

test('getEditorInitialContentConfig does not load dirty non-markdown file', () => {
    assert.deepEqual(getEditorInitialContentConfig({
        activeFile: 'src/file.ts',
        savedContent: 'saved',
        draftContent: null,
        isDirty: true,
        isMarkdownFile: false,
        isFileSwitch: false,
    }), {
        content: 'saved',
        baselineContent: 'saved',
        shouldLoad: false,
        resetHistory: false,
        reason: null,
        awaitingInitialSync: false,
    });
});

test('getEditorInitialContentConfig awaits initial sync when no saved content for non-markdown', () => {
    assert.deepEqual(getEditorInitialContentConfig({
        activeFile: 'src/file.ts',
        savedContent: null,
        draftContent: null,
        isDirty: false,
        isMarkdownFile: false,
        isFileSwitch: true,
    }), {
        content: '',
        baselineContent: '',
        shouldLoad: false,
        resetHistory: false,
        reason: null,
        awaitingInitialSync: true,
    });
});

test('getEditorInitialContentConfig loads dirty markdown file with draft', () => {
    assert.deepEqual(getEditorInitialContentConfig({
        activeFile: 'README.md',
        savedContent: 'saved',
        draftContent: 'draft',
        isDirty: true,
        isMarkdownFile: true,
        isFileSwitch: true,
    }), {
        content: 'draft',
        baselineContent: 'saved',
        shouldLoad: true,
        resetHistory: true,
        reason: 'open',
        awaitingInitialSync: false,
    });
});

test('getEditorInitialContentConfig loads dirty markdown file without draft on file switch', () => {
    assert.deepEqual(getEditorInitialContentConfig({
        activeFile: 'README.md',
        savedContent: 'saved',
        draftContent: null,
        isDirty: true,
        isMarkdownFile: true,
        isFileSwitch: true,
    }), {
        content: 'saved',
        baselineContent: 'saved',
        shouldLoad: true,
        resetHistory: true,
        reason: 'open',
        awaitingInitialSync: false,
    });
});

test('getEditorInitialContentConfig does not load dirty markdown file without draft on same file', () => {
    assert.deepEqual(getEditorInitialContentConfig({
        activeFile: 'README.md',
        savedContent: 'saved',
        draftContent: null,
        isDirty: true,
        isMarkdownFile: true,
        isFileSwitch: false,
    }), {
        content: 'saved',
        baselineContent: 'saved',
        shouldLoad: false,
        resetHistory: false,
        reason: null,
        awaitingInitialSync: false,
    });
});

test('getEditorInitialContentConfig loads clean markdown file', () => {
    assert.deepEqual(getEditorInitialContentConfig({
        activeFile: 'README.md',
        savedContent: 'saved',
        draftContent: null,
        isDirty: false,
        isMarkdownFile: true,
        isFileSwitch: true,
    }), {
        content: 'saved',
        baselineContent: 'saved',
        shouldLoad: true,
        resetHistory: true,
        reason: 'open',
        awaitingInitialSync: false,
    });
});

test('getEditorInitialContentConfig awaits initial sync when no saved content for markdown', () => {
    assert.deepEqual(getEditorInitialContentConfig({
        activeFile: 'README.md',
        savedContent: null,
        draftContent: null,
        isDirty: false,
        isMarkdownFile: true,
        isFileSwitch: true,
    }), {
        content: '',
        baselineContent: '',
        shouldLoad: false,
        resetHistory: false,
        reason: null,
        awaitingInitialSync: true,
    });
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

test('scenario: switching files restores content by exact path without leaking previous draft', () => {
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
            draftContent: undefined,
            isDirty: false,
        },
    ]);

    assert.deepEqual(getEditorContentSnapshotForPath(registry, 'src/b.ts', {
        savedContent: 'a-base',
        draftContent: 'a-draft',
        isDirty: true,
    }), {
        filePath: 'src/b.ts',
        savedContent: 'b-base',
        draftContent: undefined,
        isDirty: false,
    });
    assert.deepEqual(getEditorContentSnapshotForPath(registry, 'src/a.ts'), {
        filePath: 'src/a.ts',
        savedContent: 'a-base',
        draftContent: 'a-draft',
        isDirty: true,
    });
});

test('scenario: dirty inactive file is the only shutdown save candidate while clean active file is open', () => {
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
            draftContent: undefined,
            isDirty: false,
        },
    ]);

    assert.deepEqual(getOpenDirtyEditorSaveCandidates(registry, ['src/a.ts', 'src/b.ts']), [
        {
            path: 'src/a.ts',
            baseline: 'a-base',
            draft: 'a-draft',
            source: 'editor-buffer-registry',
        },
    ]);
});

test('scenario: same filename in different directories keeps separate drafts and candidates', () => {
    const registry = applyEditorContentSnapshots({}, [
        {
            filePath: 'src/left/file.ts',
            savedContent: 'left-base',
            draftContent: 'left-draft',
            isDirty: true,
        },
        {
            filePath: 'src/right/file.ts',
            savedContent: 'right-base',
            draftContent: 'right-draft',
            isDirty: true,
        },
    ]);

    assert.deepEqual(getEditorContentSnapshotForPath(registry, 'src/left/file.ts'), {
        filePath: 'src/left/file.ts',
        savedContent: 'left-base',
        draftContent: 'left-draft',
        isDirty: true,
    });
    assert.deepEqual(getEditorContentSnapshotForPath(registry, 'src/right/file.ts'), {
        filePath: 'src/right/file.ts',
        savedContent: 'right-base',
        draftContent: 'right-draft',
        isDirty: true,
    });
    assert.deepEqual(getOpenDirtyEditorSaveCandidates(registry, [
        'src/left/file.ts',
        'src/right/file.ts',
    ]), [
        {
            path: 'src/left/file.ts',
            baseline: 'left-base',
            draft: 'left-draft',
            source: 'editor-buffer-registry',
        },
        {
            path: 'src/right/file.ts',
            baseline: 'right-base',
            draft: 'right-draft',
            source: 'editor-buffer-registry',
        },
    ]);
});

test('scenario: stale read for file A does not affect file B', () => {
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
            draftContent: undefined,
            dirty: false,
            version: 1,
        },
    };

    // Simulate a late read for file A arriving after switching to file B
    const lateSnapshot = createEditorContentSnapshot({
        filePath: 'src/a.ts',
        baselineContent: 'a-base',
        currentContent: 'a-draft',
    });

    // Applying the late snapshot should only affect file A
    const updated = applyEditorContentSnapshot(registry, lateSnapshot);

    // File B should remain unchanged
    assert.deepEqual(updated['src/b.ts'], {
        path: 'src/b.ts',
        cleanContent: 'b-base',
        draftContent: undefined,
        dirty: false,
        version: 1,
    });

    // File A should still have its draft
    assert.deepEqual(updated['src/a.ts'], {
        path: 'src/a.ts',
        cleanContent: 'a-base',
        draftContent: 'a-draft',
        dirty: true,
        version: 1,
    });
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
