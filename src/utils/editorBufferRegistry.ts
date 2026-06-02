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

export type EditorContentSnapshotFallback = {
    savedContent?: string | null;
    draftContent?: string | null;
    isDirty?: boolean;
};

export type CreateEditorContentSnapshotInput = {
    filePath?: string;
    baselineContent: string;
    currentContent: string;
};

export type EditorContentStatePropagation = {
    immediate?: EditorContentSnapshot;
    debounced?: EditorContentSnapshot;
};

export type EditorContentMirrorState = {
    path?: string;
    savedContent?: string;
    draftContent?: string;
    isDirty?: boolean;
};

export type EditorContentTargetMirror = {
    id: string;
    path?: string;
};

export type EditorContentSnapshotMirror = EditorContentMirrorState & EditorContentTargetMirror;

export type ApplyEditorContentSnapshotToMirrorOptions = {
    mirrorDraftContent?: boolean;
    mirrorDirtySavedContent?: boolean;
};

export function normalizeEditorPath(value: string): string {
    return value.replace(/\\/g, '/').replace(/\/+/g, '/').replace(/\/$/, '');
}

export function createEditorContentSnapshot({
    filePath,
    baselineContent,
    currentContent,
}: CreateEditorContentSnapshotInput): EditorContentSnapshot {
    const isDirty = currentContent !== baselineContent;

    return {
        filePath,
        savedContent: isDirty ? baselineContent : currentContent,
        draftContent: isDirty ? currentContent : undefined,
        isDirty,
    };
}

export function getEditorContentStatePropagation(
    snapshot: EditorContentSnapshot,
    wasDirty: boolean,
): EditorContentStatePropagation {
    if (!snapshot.isDirty) {
        return { immediate: snapshot };
    }

    if (!wasDirty) {
        return {
            immediate: {
                filePath: snapshot.filePath,
                savedContent: snapshot.savedContent,
                draftContent: undefined,
                isDirty: true,
            },
            debounced: snapshot,
        };
    }

    return { debounced: snapshot };
}

export function applyEditorContentSnapshotToMirror<T extends EditorContentMirrorState>(
    mirror: T,
    snapshot: EditorContentSnapshot,
    options: ApplyEditorContentSnapshotToMirrorOptions = {},
): T {
    const shouldMirrorDirtySavedContent = options.mirrorDirtySavedContent ?? true;
    const nextSavedContent = snapshot.isDirty && !shouldMirrorDirtySavedContent
        ? mirror.savedContent
        : snapshot.savedContent ?? mirror.savedContent;
    const shouldMirrorDraftContent = options.mirrorDraftContent ?? true;
    const nextDraftContent = !snapshot.isDirty
        ? undefined
        : shouldMirrorDraftContent
            ? snapshot.draftContent ?? mirror.draftContent
            : undefined;

    if (
        mirror.savedContent === nextSavedContent
        && mirror.draftContent === nextDraftContent
        && Boolean(mirror.isDirty) === snapshot.isDirty
    ) {
        return mirror;
    }

    return {
        ...mirror,
        savedContent: nextSavedContent,
        draftContent: nextDraftContent,
        isDirty: snapshot.isDirty,
    };
}

export function getEditorContentSnapshotFromMirror(
    mirror: EditorContentMirrorState,
    hasRegistryBuffer: boolean,
): EditorContentSnapshot | null {
    if (
        !mirror.path
        || (!mirror.isDirty && mirror.savedContent === undefined && mirror.draftContent === undefined)
    ) {
        return null;
    }

    return {
        filePath: mirror.path,
        savedContent: hasRegistryBuffer ? undefined : mirror.savedContent,
        draftContent: hasRegistryBuffer ? undefined : mirror.isDirty ? mirror.draftContent : undefined,
        isDirty: Boolean(mirror.isDirty),
    };
}

export function getEditorContentSnapshotsFromMirrors<T extends EditorContentSnapshotMirror>(
    registry: EditorBufferRegistry,
    mirrors: T[],
    excludedMirrorId: string | null = null,
): EditorContentSnapshot[] {
    return mirrors.flatMap((mirror): EditorContentSnapshot[] => {
        if (mirror.id === excludedMirrorId) {
            return [];
        }

        const snapshot = getEditorContentSnapshotFromMirror(
            mirror,
            Boolean(mirror.path && registry[normalizeEditorPath(mirror.path)]),
        );
        return snapshot ? [snapshot] : [];
    });
}

export function getEditorContentSnapshotTargetId<T extends EditorContentTargetMirror>(
    mirrors: T[],
    fallbackMirrorId: string | null,
    snapshot: EditorContentSnapshot,
): string | null {
    if (!snapshot.filePath) {
        return fallbackMirrorId;
    }

    const targetPath = normalizeEditorPath(snapshot.filePath);
    return mirrors.find(mirror => (
        mirror.path
        && normalizeEditorPath(mirror.path) === targetPath
    ))?.id ?? null;
}

export function shouldFlushEditorContentSnapshotImmediately(snapshot: EditorContentSnapshot): boolean {
    return !snapshot.isDirty || snapshot.draftContent === undefined;
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

export function getOpenDirtyEditorSaveCandidates(
    registry: EditorBufferRegistry,
    openPaths: string[],
): DirtyEditorSaveCandidate[] {
    const openPathSet = new Set(openPaths.map(normalizeEditorPath));
    return getDirtyEditorSaveCandidates(registry)
        .filter(candidate => openPathSet.has(normalizeEditorPath(candidate.path)));
}

export function markEditorSaveCandidatesClean(
    registry: EditorBufferRegistry,
    candidates: DirtyEditorSaveCandidate[],
): EditorBufferRegistry {
    return applyEditorContentSnapshots(
        registry,
        candidates.map(candidate => ({
            filePath: candidate.path,
            savedContent: candidate.draft,
            draftContent: undefined,
            isDirty: false,
        })),
    );
}

export function getEditorContentSnapshotForPath(
    registry: EditorBufferRegistry,
    path: string | undefined | null,
    fallback: EditorContentSnapshotFallback = {},
): EditorContentSnapshot {
    const buffer = path ? registry[normalizeEditorPath(path)] : undefined;
    if (buffer) {
        return {
            filePath: path ?? undefined,
            savedContent: buffer.cleanContent,
            draftContent: buffer.draftContent,
            isDirty: buffer.dirty,
        };
    }

    return {
        filePath: path ?? undefined,
        savedContent: fallback.savedContent ?? undefined,
        draftContent: fallback.draftContent ?? undefined,
        isDirty: Boolean(fallback.isDirty),
    };
}

export function getEditorContentSnapshotForMirror(
    registry: EditorBufferRegistry,
    mirror: EditorContentMirrorState,
): EditorContentSnapshot {
    return getEditorContentSnapshotForPath(registry, mirror.path, {
        savedContent: mirror.savedContent,
        draftContent: mirror.draftContent,
        isDirty: mirror.isDirty,
    });
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
