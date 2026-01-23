import { useEffect, useRef, useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

export interface TabState {
    id: string;
    title: string;
    type: string;
    path?: string;
}

export interface TerminalState {
    id: string;
    title: string;
    cwd?: string;
}

export interface ProjectState {
    project_path: string;
    active_file: string | null;
    open_tabs: TabState[];
    selected_model_id: string | null;
    terminals: TerminalState[];
    active_terminal_id: string | null;
    terminal_height: number | null;
    chat_panel_width: number | null;
    explorer_width: number | null;
}

interface UseProjectStateOptions {
    projectPath: string | null;
    tabs: TabState[];
    activeTabId: string | null;
    selectedModelId: string;
    terminals: TerminalState[];
    activeTerminalId: string;
    terminalHeight: number;
    onStateLoaded?: (state: ProjectState) => void;
}

interface UseProjectStateReturn {
    saveState: () => Promise<void>;
    loadState: () => Promise<ProjectState | null>;
    loaded: boolean;
    isClosing: boolean;
}

export function useProjectState({
    projectPath,
    tabs,
    activeTabId,
    selectedModelId,
    terminals,
    activeTerminalId,
    terminalHeight,
    onStateLoaded,
}: UseProjectStateOptions): UseProjectStateReturn {
    const [loaded, setLoaded] = useState(false);
    const [isClosing, setIsClosing] = useState(false);

    // Refs for state tracking
    const isClosingRef = useRef(false);
    const isDirtyRef = useRef(false);
    const isRestoringRef = useRef(false);
    const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    // Helper to construct state object
    const constructState = useCallback((): ProjectState | null => {
        if (!projectPath) return null;

        const activeTab = tabs.find(t => t.id === activeTabId);
        const activeFile = activeTab?.type === 'file' ? activeTab.path || null : null;

        return {
            project_path: projectPath,
            active_file: activeFile,
            open_tabs: tabs.filter(t => t.type === 'file').map(t => ({
                id: t.id,
                title: t.title,
                type: t.type,
                path: t.path,
            })),
            selected_model_id: selectedModelId,
            terminals: terminals.map(t => ({
                id: t.id,
                title: t.title,
                cwd: t.cwd,
            })),
            active_terminal_id: activeTerminalId,
            terminal_height: terminalHeight,
            // These UI dimensions are currently not tracked in React state in a way we can easily retrieve here
            // They would need to be passed in props if we want to persist them accurately
            chat_panel_width: null,
            explorer_width: null,
        };
    }, [projectPath, tabs, activeTabId, selectedModelId, terminals, activeTerminalId, terminalHeight]);

    const saveState = useCallback(async () => {
        if (!projectPath) return;

        // Optimization: Don't save if state hasn't changed (clean)
        if (!isDirtyRef.current) {
            return;
        }

        const state = constructState();
        if (!state) return;

        try {
            await invoke('save_project_state', { stateData: state });
            // console.log('[ProjectState] Saved state for:', projectPath); // Disabled log for cleaner output
            isDirtyRef.current = false;
        } catch (e) {
            console.error('[ProjectState] Failed to save state:', e);
        }
    }, [projectPath, constructState]);

    const loadState = useCallback(async (): Promise<ProjectState | null> => {
        if (!projectPath) return null;

        try {
            const state = await invoke<ProjectState | null>('load_project_state', { projectPath });
            // console.log('[ProjectState] Loaded state invoked'); // Disabled log
            return state;
        } catch (e) {
            console.error('[ProjectState] Failed to load state:', e);
            return null;
        }
    }, [projectPath]);

    // Initial Load Effect
    useEffect(() => {
        if (!projectPath || loaded) return;

        const load = async () => {
            // Flag that we are restoring state to prevent dirty-marking
            isRestoringRef.current = true;

            const state = await loadState();

            if (state && onStateLoaded) {
                onStateLoaded(state);
            } else {
                console.log('[ProjectState] No saved state found');
            }

            setLoaded(true);

            // Allow a small tick for React state updates to settle before enabling dirty tracking
            setTimeout(() => {
                isRestoringRef.current = false;
            }, 100);
        };

        load();
    }, [projectPath, loadState, onStateLoaded, loaded]);

    // Change Detection Effect
    useEffect(() => {
        if (!projectPath || !loaded) return;

        // If we represent a change but we are currently restoring, ignore it
        if (isRestoringRef.current) return;

        // Mark as dirty
        isDirtyRef.current = true;

        // Debounce save
        if (saveTimeoutRef.current) {
            clearTimeout(saveTimeoutRef.current);
        }

        saveTimeoutRef.current = setTimeout(() => {
            saveState();
        }, 1000);

        return () => {
            if (saveTimeoutRef.current) {
                clearTimeout(saveTimeoutRef.current);
            }
        };
    }, [projectPath, tabs, activeTabId, selectedModelId, terminals, activeTerminalId, terminalHeight, loaded, saveState]);

    // Exit Handler - The "Deep Fix"
    // We delegate the exit-save-sequence entirely to the backend
    useEffect(() => {
        let unlisten: (() => void) | undefined;

        const setupListener = async () => {
            const currentWindow = getCurrentWindow();

            unlisten = await currentWindow.onCloseRequested(async (event) => {
                // Ignore if already closing
                if (isClosingRef.current) {
                    event.preventDefault();
                    return;
                }

                if (projectPath && loaded) {
                    event.preventDefault(); // Stop immediate close

                    isClosingRef.current = true;
                    setIsClosing(true); // Trigger UI overlay

                    // Construct final state
                    const finalState = constructState();

                    // One-way ticket: Verify state and signal backend to take over
                    if (finalState) {
                        // We DO NOT await this. We intentionally fire-and-forget from the frontend perspective
                        // because the backend command 'graceful_shutdown_with_state' will:
                        // 1. Save the state synchronously
                        // 2. Call app_handle.exit() effectively killing the app
                        // This avoids any frontend event-loop blocking or IPC response issues.
                        invoke('graceful_shutdown_with_state', { stateData: finalState })
                            .catch(e => {
                                console.error('Shutdown failed:', e);
                                // Fallback force exit if backend command somehow fails to kill app
                                currentWindow.destroy();
                            });
                    } else {
                        // If no state to save, just destroy
                        currentWindow.destroy();
                    }
                }
            });
        };

        setupListener();

        return () => {
            if (unlisten) unlisten();
        };
    }, [projectPath, loaded, constructState]);

    return { saveState, loadState, loaded, isClosing };
}
