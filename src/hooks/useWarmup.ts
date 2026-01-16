import { useCallback, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface WarmupResponse {
    response_type: string;
    session_id: string;
    provider: string;
    cache_supported: boolean;
    artifacts_loaded: number;
    cache_ready: boolean;
    duration_ms: number;
    message?: string;
}

export type WarmupTrigger = 'launch' | 'model_change' | 'workspace_change' | 'session_resume';

const INACTIVITY_THRESHOLD = 5 * 60 * 1000; // 5 minutes

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
 */
export function useWarmup(workspacePath: string | null, modelId: string | null, ready: boolean = false) {
    const lastActivityRef = useRef<number>(Date.now());
    const hasWarmedOnLaunchRef = useRef<boolean>(false);
    const lastModelRef = useRef<string | null>(null);
    const lastWorkspaceRef = useRef<string | null>(null);
    const isReadyRef = useRef<boolean>(false);

    const sessionId = generateSessionId(workspacePath);

    const warmup = useCallback(async (trigger: WarmupTrigger): Promise<WarmupResponse | null> => {
        if (!modelId) {
            console.log('[Warmup] Skipping - no model selected');
            return null;
        }

        try {
            console.log(`[Warmup] Sending warmup request: trigger=${trigger}, model=${modelId}, session=${sessionId}`);
            const response = await invoke<WarmupResponse>('warmup_cache', {
                sessionId,
                model: modelId,
                trigger,
            });

            console.log(`[Warmup] Response: type=${response.response_type}, provider=${response.provider}, artifacts=${response.artifacts_loaded}, ready=${response.cache_ready}`);

            if (response.message) {
                console.log(`[Warmup] Message: ${response.message}`);
            }

            return response;
        } catch (error) {
            // Warmup failures are non-fatal
            console.warn('[Warmup] Failed (non-fatal):', error);
            return null;
        }
    }, [sessionId, modelId]);

    // Track when we become ready (state loaded)
    useEffect(() => {
        if (ready && !isReadyRef.current) {
            isReadyRef.current = true;
            // Initialize refs with current values to prevent false "change" triggers
            lastModelRef.current = modelId;
            lastWorkspaceRef.current = workspacePath;
        }
    }, [ready, modelId, workspacePath]);

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
            console.log(`[Warmup] Model changed: ${lastModelRef.current} -> ${modelId}`);
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
            console.log(`[Warmup] Workspace changed: ${lastWorkspaceRef.current} -> ${workspacePath}`);
            warmup('workspace_change');
        }
        if (isReadyRef.current) {
            lastWorkspaceRef.current = workspacePath;
        }
    }, [ready, workspacePath, warmup]);

    // Check for session resume (after inactivity)
    const checkSessionResume = useCallback(async () => {
        const now = Date.now();
        const inactiveDuration = now - lastActivityRef.current;

        if (inactiveDuration > INACTIVITY_THRESHOLD) {
            // Check if backend also thinks we should rewarm
            try {
                const shouldRewarm = await invoke<boolean>('should_rewarm_cache');
                if (shouldRewarm) {
                    console.log('[Warmup] Session resume detected after inactivity');
                    warmup('session_resume');
                }
            } catch (error) {
                console.warn('[Warmup] Failed to check rewarm status:', error);
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
