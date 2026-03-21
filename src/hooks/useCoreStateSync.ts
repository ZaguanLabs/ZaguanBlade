/**
 * Core State Sync Hook
 *
 * Provides state recovery and synchronization with the Rust backend.
 * Used for UI initialization, reload recovery, and ensuring state consistency.
 *
 * Part of the headless architecture migration.
 */

import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useOptionalStartupBootstrap } from '../contexts/StartupBootstrapContext';
import type { BootstrapState } from '../types/bootstrap';
import type { CoreStateSnapshot, FeatureFlagsSnapshot } from '../types/coreState';
import type { BladeEventEnvelope, EditorEvent } from '../types/blade';

export interface CoreStateSyncResult {
    /** Whether initial state recovery is in progress */
    isRecovering: boolean;
    /** The last recovered core state snapshot */
    coreState: CoreStateSnapshot | null;
    /** Current feature flags */
    featureFlags: FeatureFlagsSnapshot | null;
    /** Error if recovery failed */
    error: string | null;
    /** Manually trigger state recovery */
    recover: () => Promise<void>;
}

/**
 * Hook for synchronizing with backend core state.
 * 
 * On mount:
 * 1. Loads feature flags
 * 2. Fetches core state snapshot
 * 3. Dispatches 'core-state-recovered' event for other components
 * 
 * Also listens for editor state events to keep local cache updated.
 */
export function useCoreStateSync(): CoreStateSyncResult {
    const bootstrapContext = useOptionalStartupBootstrap();
    const [isRecovering, setIsRecovering] = useState(true);
    const [coreState, setCoreState] = useState<CoreStateSnapshot | null>(null);
    const [featureFlags, setFeatureFlags] = useState<FeatureFlagsSnapshot | null>(null);
    const [error, setError] = useState<string | null>(null);

    const applyBootstrap = useCallback((bootstrap: BootstrapState) => {
        const state = bootstrap.core_state;
        const flags = bootstrap.feature_flags;

        setFeatureFlags(flags);
        setCoreState(state);
        setError(null);
        setIsRecovering(false);

        window.dispatchEvent(new CustomEvent('core-state-recovered', {
            detail: { state, flags }
        }));

        console.debug('[CoreStateSync] Recovery complete:', {
            workspace: state.workspace.path,
            activeFile: state.editor.active_file,
            openFiles: state.editor.open_files.length,
            messageCount: state.chat.message_count,
            capabilities: state.protocol.capabilities,
        });
    }, []);

    const recover = useCallback(async () => {
        setIsRecovering(true);
        setError(null);

        try {
            const bootstrap = bootstrapContext?.bootstrap
                ?? await invoke<BootstrapState>('bootstrap_state');
            applyBootstrap(bootstrap);
        } catch (e) {
            const message = e instanceof Error ? e.message : String(e);
            console.error('[CoreStateSync] Recovery failed:', message);
            setError(message);
            setIsRecovering(false);
        }
    }, [applyBootstrap, bootstrapContext?.bootstrap]);

    // Initial recovery on mount
    useEffect(() => {
        if (bootstrapContext?.bootstrap) {
            void recover();
            return;
        }

        if (bootstrapContext?.isLoading) {
            return;
        }

        void recover();
    }, [bootstrapContext?.bootstrap, bootstrapContext?.isLoading, recover]);

    // Listen for editor state events to keep cache updated
    useEffect(() => {
        const unlistenPromise = listen<BladeEventEnvelope>('blade-event', (event) => {
                const bladeEvent = event.payload.event;
                
                if (bladeEvent.type === 'Editor') {
                    const editorEvent = bladeEvent.payload as EditorEvent;
                    
                    // Update local cache based on event type
                    if (editorEvent.type === 'StateSnapshot') {
                        setCoreState(prev => prev ? {
                            ...prev,
                            editor: {
                                active_file: editorEvent.payload.active_file,
                                open_files: editorEvent.payload.open_files,
                                cursor_line: editorEvent.payload.cursor_line,
                                cursor_column: editorEvent.payload.cursor_column,
                                selection_start: editorEvent.payload.selection_start,
                                selection_end: editorEvent.payload.selection_end,
                            }
                        } : null);
                    } else if (editorEvent.type === 'ActiveFileChanged') {
                        setCoreState(prev => prev ? {
                            ...prev,
                            editor: {
                                ...prev.editor,
                                active_file: editorEvent.payload.path,
                            }
                        } : null);
                    } else if (editorEvent.type === 'FileOpened') {
                        setCoreState(prev => {
                            if (!prev) return null;
                            const openFiles = prev.editor.open_files.includes(editorEvent.payload.path)
                                ? prev.editor.open_files
                                : [...prev.editor.open_files, editorEvent.payload.path];
                            return {
                                ...prev,
                                editor: {
                                    ...prev.editor,
                                    open_files: openFiles,
                                }
                            };
                        });
                    } else if (editorEvent.type === 'FileClosed') {
                        setCoreState(prev => prev ? {
                            ...prev,
                            editor: {
                                ...prev.editor,
                                open_files: prev.editor.open_files.filter(f => f !== editorEvent.payload.path),
                            }
                        } : null);
                    }
                }
            });

        return () => {
            unlistenPromise
                .then(unlisten => unlisten())
                .catch(console.error);
        };
    }, []);

    return {
        isRecovering,
        coreState,
        featureFlags,
        error,
        recover,
    };
}

/**
 * Hook to listen for core state recovery events.
 * Useful for components that need to react to state recovery.
 */
export function useOnCoreStateRecovered(
    callback: (state: CoreStateSnapshot, flags: FeatureFlagsSnapshot) => void
) {
    useEffect(() => {
        const handler = (event: CustomEvent<{ state: CoreStateSnapshot; flags: FeatureFlagsSnapshot }>) => {
            callback(event.detail.state, event.detail.flags);
        };

        window.addEventListener('core-state-recovered', handler as EventListener);
        return () => {
            window.removeEventListener('core-state-recovered', handler as EventListener);
        };
    }, [callback]);
}
