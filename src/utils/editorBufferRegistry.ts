export type EditorContentSnapshot = {
    filePath?: string;
    savedContent?: string;
    draftContent?: string;
    isDirty: boolean;
};

export type EditorBufferState = {
    path: string;
    cleanContent: string;
    draftContent?: string;
    dirty: boolean;
    version: number;
};

export type EditorBufferRegistry = Record<string, EditorBufferState>;

export type DirtyEditorSaveCandidate = {
    path: string;
    baseline: string;
    draft: string;
    source: 'editor-buffer-registry';
};

export function normalizeEditorPath(value: string): string {
    return value.replace(/\\/g, '/').replace(/\/+/g, '/').replace(/\/$/, '');
}

export function applyEditorContentSnapshot(
    registry: EditorBufferRegistry,
    snapshot: EditorContentSnapshot,
): EditorBufferRegistry {
    if (!snapshot.filePath) {
        return registry;
    }

    const path = normalizeEditorPath(snapshot.filePath);
    const previous = registry[path];
    const cleanContent = snapshot.savedContent ?? previous?.cleanContent ?? '';
    const draftContent = snapshot.isDirty
        ? snapshot.draftContent ?? previous?.draftContent
        : undefined;

    const next: EditorBufferState = {
        path,
        cleanContent,
        draftContent,
        dirty: snapshot.isDirty,
        version: (previous?.version ?? 0) + 1,
    };

    if (
        previous
        && previous.cleanContent === next.cleanContent
        && previous.draftContent === next.draftContent
        && previous.dirty === next.dirty
    ) {
        return registry;
    }

    return {
        ...registry,
        [path]: next,
    };
}

export function applyEditorContentSnapshots(
    registry: EditorBufferRegistry,
    snapshots: EditorContentSnapshot[],
): EditorBufferRegistry {
    return snapshots.reduce(applyEditorContentSnapshot, registry);
}

export function getDirtyEditorSaveCandidates(
    registry: EditorBufferRegistry,
): DirtyEditorSaveCandidate[] {
    return Object.values(registry)
        .filter((buffer) => buffer.dirty && buffer.draftContent !== undefined)
        .map((buffer) => ({
            path: buffer.path,
            baseline: buffer.cleanContent,
            draft: buffer.draftContent ?? '',
            source: 'editor-buffer-registry' as const,
        }));
}

export function pruneEditorBufferRegistry(
    registry: EditorBufferRegistry,
    retainedPaths: string[],
): EditorBufferRegistry {
    const retained = new Set(retainedPaths.map(normalizeEditorPath));
    const entries = Object.entries(registry).filter(([path]) => retained.has(path));

    if (entries.length === Object.keys(registry).length) {
        return registry;
    }

    return Object.fromEntries(entries);
}
