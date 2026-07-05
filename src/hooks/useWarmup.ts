import { useCallback, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface WarmupResponse {
    response_type: string;
    session_id: string;
    provider: string;
    cache_supported: boolean;
    artifacts_loaded: number;
    cache_ready: boolean;
    duration_ms: number;
    message?: string;
}

type WarmupTrigger =
    | 'launch'
    | 'model_change'
    | 'workspace_change'
    | 'session_resume'
    | 'active_file_change'
    | 'file_save'
    | 'guidance_change';

/** Mirrors `warmup_bundle::EditorWarmupSnapshot` (camelCase serde) in Rust. */
export interface WarmupPosition {
    line: number;
    column: number;
}

export interface WarmupRange {
    start: WarmupPosition;
    end: WarmupPosition;
}

export interface WarmupOpenFile {
    path: string;
    isModified: boolean;
    tabOrder: number;
}

export interface EditorWarmupSnapshot {
    activeFile: string | null;
    cursor: WarmupPosition | null;
    selection: WarmupRange | null;
    visibleRange: WarmupRange | null;
    openFiles: WarmupOpenFile[];
    /** Buffer content of the active file, only when it has unsaved changes. */
    activeBufferContent: string | null;
}

/**
 * Editor context for warmup context prefetch. `activeFilePath` and
 * `activeFileDirty` drive the editor triggers; `getSnapshot` is called at
 * send time so every warmup carries the current editor state.
 */
export interface WarmupEditorContext {
    activeFilePath: string | null;
    activeFileDirty: boolean;
    getSnapshot: () => EditorWarmupSnapshot;
}

const INACTIVITY_THRESHOLD = 5 * 60 * 1000; // 5 minutes
// Contract allows 2-5s; rapid typing/saving collapses to one send. The
// backend additionally dedupes byte-identical bundles, so this only needs
// to stop bursts, not enforce exactness.
const EDITOR_TRIGGER_DEBOUNCE_MS = 3000;

function isLocalModel(modelId: string | null): boolean {
    if (!modelId) {
        return false;
    }

    return modelId.startsWith('ollama/') || modelId.startsWith('openai-compat/');
}

function isGuidanceFile(path: string): boolean {
    return /(^|[/\\])AGENTS\.md$/i.test(path);
}

/**
 * Generate a session ID from workspace path
 * This creates a stable session ID for a given workspace
 */
function generateSessionId(workspacePath: string | null): string {
    if (!workspacePath) return 'default';
    // Simple hash of workspace path for stable session ID
    let hash = 0;
    for (let i = 0; i < workspacePath.length; i++) {
        const char = workspacePath.charCodeAt(i);
        hash = ((hash << 5) - hash) + char;
        hash = hash & hash; // Convert to 32bit integer
    }
    return `sess_${Math.abs(hash).toString(16)}`;
}

/**
 * Cache warmup hook for Blade Protocol v2.1
 *
 * @param workspacePath - Current workspace path
 * @param modelId - Currently selected model ID
 * @param ready - Whether the app state has fully loaded (project state restored)
 *                Set to true only after project state has been loaded to avoid
 *                multiple warmups during initialization
 * @param editor - Editor context for warmup context prefetch. When provided,
 *                 every warmup carries an editor snapshot, and active-file
 *                 changes / saves fire debounced warmups of their own.
 */
export function useWarmup(
    workspacePath: string | null,
    modelId: string | null,
    ready: boolean = false,
    editor?: WarmupEditorContext,
) {
    const lastActivityRef = useRef<number>(Date.now());
    const hasWarmedOnLaunchRef = useRef<boolean>(false);
    const lastModelRef = useRef<string | null>(null);
    const lastWorkspaceRef = useRef<string | null>(null);
    const isReadyRef = useRef<boolean>(false);
    const lastActiveFileRef = useRef<string | null>(null);
    const lastActiveDirtyRef = useRef<boolean>(false);
    const editorDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    // Snapshot provider behind a ref: warmup() always reads the freshest
    // editor state without retriggering the callback/effect chain.
    const getSnapshotRef = useRef<(() => EditorWarmupSnapshot) | null>(null);
    getSnapshotRef.current = editor?.getSnapshot ?? null;

    const activeFilePath = editor?.activeFilePath ?? null;
    const activeFileDirty = editor?.activeFileDirty ?? false;

    const sessionId = generateSessionId(workspacePath);

    const warmup = useCallback(async (trigger: WarmupTrigger): Promise<WarmupResponse | null> => {
        if (!modelId || isLocalModel(modelId)) {
            return null;
        }

        try {
            const response = await invoke<WarmupResponse>('warmup_cache', {
                sessionId,
                model: modelId,
                trigger,
                editorSnapshot: getSnapshotRef.current?.() ?? null,
            });

            return response;
        } catch (error) {
            // Warmup failures are non-fatal
            void error;
            return null;
        }
    }, [sessionId, modelId]);

    // Debounced warmup for editor-driven triggers. One shared timer: the
    // latest trigger wins, since the bundle always carries the full current
    // editor state regardless of which event scheduled it.
    const scheduleEditorWarmup = useCallback((trigger: WarmupTrigger) => {
        if (editorDebounceRef.current) {
            clearTimeout(editorDebounceRef.current);
        }
        editorDebounceRef.current = setTimeout(() => {
            editorDebounceRef.current = null;
            warmup(trigger);
        }, EDITOR_TRIGGER_DEBOUNCE_MS);
    }, [warmup]);

    useEffect(() => {
        return () => {
            if (editorDebounceRef.current) {
                clearTimeout(editorDebounceRef.current);
                editorDebounceRef.current = null;
            }
        };
    }, []);

    // Track when we become ready (state loaded)
    useEffect(() => {
        if (ready && !isReadyRef.current) {
            isReadyRef.current = true;
            // Initialize refs with current values to prevent false "change" triggers
            lastModelRef.current = modelId;
            lastWorkspaceRef.current = workspacePath;
            lastActiveFileRef.current = activeFilePath;
            lastActiveDirtyRef.current = activeFileDirty;
        }
    }, [ready, modelId, workspacePath, activeFilePath, activeFileDirty]);

    // Warmup on launch (once, only after ready)
    useEffect(() => {
        if (ready && modelId && !hasWarmedOnLaunchRef.current) {
            hasWarmedOnLaunchRef.current = true;
            warmup('launch');
        }
    }, [ready, modelId, warmup]);

    // Warmup on model change (only after ready, to avoid warmups during init)
    useEffect(() => {
        // Only trigger model change warmup if we're ready and this is a real change
        // Also require that we've already done the launch warmup to avoid race conditions
        if (ready && isReadyRef.current && hasWarmedOnLaunchRef.current && modelId && lastModelRef.current && lastModelRef.current !== modelId) {
            warmup('model_change');
        }
        // Always update the ref (but only matters after ready)
        if (isReadyRef.current) {
            lastModelRef.current = modelId;
        }
    }, [ready, modelId, warmup]);

    // Warmup on workspace change (only after ready)
    useEffect(() => {
        if (ready && isReadyRef.current && workspacePath && lastWorkspaceRef.current && lastWorkspaceRef.current !== workspacePath) {
            warmup('workspace_change');
        }
        if (isReadyRef.current) {
            lastWorkspaceRef.current = workspacePath;
        }
    }, [ready, workspacePath, warmup]);

    // Editor triggers: active-file switch, and dirty->clean transitions (a
    // save, or an undo back to baseline — either way the shippable content
    // changed). AGENTS.md saves count as guidance changes. All debounced;
    // byte-identical outcomes are additionally deduped backend-side.
    useEffect(() => {
        if (!editor) {
            return;
        }
        const previousFile = lastActiveFileRef.current;
        const previousDirty = lastActiveDirtyRef.current;
        lastActiveFileRef.current = activeFilePath;
        lastActiveDirtyRef.current = activeFileDirty;

        if (!ready || !isReadyRef.current || !hasWarmedOnLaunchRef.current) {
            return; // init/restore churn is not a real editor change
        }

        if (activeFilePath !== previousFile) {
            scheduleEditorWarmup('active_file_change');
            return;
        }
        if (previousDirty && !activeFileDirty && activeFilePath) {
            scheduleEditorWarmup(isGuidanceFile(activeFilePath) ? 'guidance_change' : 'file_save');
        }
    }, [ready, editor, activeFilePath, activeFileDirty, scheduleEditorWarmup]);

    // Check for session resume (after inactivity)
    const checkSessionResume = useCallback(async () => {
        const now = Date.now();
        const inactiveDuration = now - lastActivityRef.current;

        if (inactiveDuration > INACTIVITY_THRESHOLD) {
            // Check if backend also thinks we should rewarm
            try {
                const shouldRewarm = await invoke<boolean>('should_rewarm_cache');
                if (shouldRewarm) {
                    warmup('session_resume');
                }
            } catch (error) {
                void error;
            }
        }

        lastActivityRef.current = now;
    }, [warmup]);

    // Track user activity - call this on user interactions
    const trackActivity = useCallback(() => {
        const now = Date.now();
        const inactiveDuration = now - lastActivityRef.current;

        // If returning from inactivity, check for session resume
        if (inactiveDuration > INACTIVITY_THRESHOLD) {
            checkSessionResume();
        } else {
            lastActivityRef.current = now;
        }
    }, [checkSessionResume]);

    return {
        warmup,
        trackActivity,
        sessionId,
    };
}
