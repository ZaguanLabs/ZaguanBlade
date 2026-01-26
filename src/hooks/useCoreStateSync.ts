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
import { listen, UnlistenFn } from '@tauri-apps/api/event';
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
    const [isRecovering, setIsRecovering] = useState(true);
    const [coreState, setCoreState] = useState<CoreStateSnapshot | null>(null);
    const [featureFlags, setFeatureFlags] = useState<FeatureFlagsSnapshot | null>(null);
    const [error, setError] = useState<string | null>(null);

    const recover = useCallback(async () => {
        setIsRecovering(true);
        setError(null);

        try {
            // Load feature flags first
            const flags = await invoke<FeatureFlagsSnapshot>('get_feature_flags');
            setFeatureFlags(flags);

            // Then load core state
            const state = await invoke<CoreStateSnapshot>('get_core_state');
            setCoreState(state);

            // Dispatch custom event for other components to react
            window.dispatchEvent(new CustomEvent('core-state-recovered', {
                detail: { state, flags }
            }));

            console.log('[CoreStateSync] Recovery complete:', {
                workspace: state.workspace.path,
                activeFile: state.editor.active_file,
                openFiles: state.editor.open_files.length,
                messageCount: state.chat.message_count,
                capabilities: state.protocol.capabilities,
            });
        } catch (e) {
            const message = e instanceof Error ? e.message : String(e);
            console.error('[CoreStateSync] Recovery failed:', message);
            setError(message);
        } finally {
            setIsRecovering(false);
        }
    }, []);

    // Initial recovery on mount
    useEffect(() => {
        recover();
    }, [recover]);

    // Listen for editor state events to keep cache updated
    useEffect(() => {
        let unlisten: UnlistenFn | undefined;

        const setup = async () => {
            unlisten = await listen<BladeEventEnvelope>('blade-event', (event) => {
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
        };

        setup();

        return () => {
            if (unlisten) unlisten();
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
