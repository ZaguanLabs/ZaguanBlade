import { useEffect, useRef, useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

interface TabState {
    id: string;
    title: string;
    type: string;
    path?: string;
}

interface TerminalState {
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
    chatPanelWidth: number;
    onStateLoaded?: (state: ProjectState) => void;
    beforeShutdown?: () => Promise<boolean>;
}

interface UseProjectStateReturn {
    saveState: () => Promise<void>;
    loadState: () => Promise<ProjectState | null>;
    loaded: boolean;
    isClosing: boolean;
}

interface ProjectStateInputs {
    projectPath: string | null;
    tabs: TabState[];
    activeTabId: string | null;
    selectedModelId: string;
    terminals: TerminalState[];
    activeTerminalId: string;
    terminalHeight: number;
    chatPanelWidth: number;
}

function serializeProjectState(state: ProjectState): string {
    return JSON.stringify(state);
}

export function useProjectState({
    projectPath,
    tabs,
    activeTabId,
    selectedModelId,
    terminals,
    activeTerminalId,
    terminalHeight,
    chatPanelWidth,
    onStateLoaded,
    beforeShutdown,
}: UseProjectStateOptions): UseProjectStateReturn {
    const [loaded, setLoaded] = useState(false);
    const [isClosing, setIsClosing] = useState(false);

    // Refs for state tracking
    const isClosingRef = useRef(false);
    const isDirtyRef = useRef(false);
    const isRestoringRef = useRef(false);
    const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const lastSavedSignatureRef = useRef<string | null>(null);
    const lastObservedSignatureRef = useRef<string | null>(null);
    const beforeShutdownRef = useRef(beforeShutdown);
    const projectStateInputsRef = useRef<ProjectStateInputs>({
        projectPath,
        tabs,
        activeTabId,
        selectedModelId,
        terminals,
        activeTerminalId,
        terminalHeight,
        chatPanelWidth,
    });

    beforeShutdownRef.current = beforeShutdown;
    projectStateInputsRef.current = {
        projectPath,
        tabs,
        activeTabId,
        selectedModelId,
        terminals,
        activeTerminalId,
        terminalHeight,
        chatPanelWidth,
    };

    // Helper to construct state object
    const constructState = useCallback((): ProjectState | null => {
        const current = projectStateInputsRef.current;
        if (!current.projectPath) return null;

        const activeTab = current.tabs.find(t => t.id === current.activeTabId);
        const activeFile = activeTab?.type === 'file' ? activeTab.path || null : null;

        return {
            project_path: current.projectPath,
            active_file: activeFile,
            open_tabs: current.tabs.filter(t => t.type === 'file').map(t => ({
                id: t.id,
                title: t.title,
                type: t.type,
                path: t.path,
            })),
            selected_model_id: current.selectedModelId,
            terminals: current.terminals.map(t => ({
                id: t.id,
                title: t.title,
                cwd: t.cwd,
            })),
            active_terminal_id: current.activeTerminalId,
            terminal_height: current.terminalHeight,
            chat_panel_width: current.chatPanelWidth,
            explorer_width: null,
        };
    }, []);

    const saveState = useCallback(async () => {
        if (!isDirtyRef.current) {
            return;
        }

        const state = constructState();
        if (!state) return;

        const signature = serializeProjectState(state);
        if (signature === lastSavedSignatureRef.current) {
            isDirtyRef.current = false;
            return;
        }

        try {
            await invoke('save_project_state', { stateData: state });
            lastSavedSignatureRef.current = signature;
            lastObservedSignatureRef.current = signature;
            isDirtyRef.current = false;
        } catch (e) {
            console.error('[ProjectState] Failed to save state:', e);
        }
    }, [constructState]);

    const loadState = useCallback(async (): Promise<ProjectState | null> => {
        if (!projectPath) return null;

        try {
            const state = await invoke<ProjectState | null>('load_project_state', { projectPath });
            // console.debug('[ProjectState] Loaded state invoked'); // Disabled log
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
            isRestoringRef.current = true;

            const state = await loadState();

            if (state && onStateLoaded) {
                const signature = serializeProjectState(state);
                lastSavedSignatureRef.current = signature;
                lastObservedSignatureRef.current = signature;
                onStateLoaded(state);
            } else {
                console.debug('[ProjectState] No saved state found');
                lastSavedSignatureRef.current = null;
                lastObservedSignatureRef.current = null;
            }

            setLoaded(true);

            setTimeout(() => {
                isRestoringRef.current = false;
            }, 100);
        };

        load();
    }, [projectPath, loadState, onStateLoaded, loaded]);

    // Change Detection Effect
    useEffect(() => {
        if (!projectPath || !loaded) return;

        if (isRestoringRef.current) return;

        const state = constructState();
        if (!state) return;

        const signature = serializeProjectState(state);
        if (signature === lastObservedSignatureRef.current) {
            return;
        }

        lastObservedSignatureRef.current = signature;

        if (signature === lastSavedSignatureRef.current) {
            isDirtyRef.current = false;
            return;
        }

        isDirtyRef.current = true;

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
    }, [projectPath, tabs, activeTabId, selectedModelId, terminals, activeTerminalId, terminalHeight, chatPanelWidth, loaded, saveState, constructState]);

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

                    try {
                        const shouldContinue = await beforeShutdownRef.current?.();
                        if (shouldContinue === false) {
                            isClosingRef.current = false;
                            setIsClosing(false);
                            return;
                        }
                    } catch (e) {
                        console.error('Before shutdown hook failed:', e);
                        isClosingRef.current = false;
                        setIsClosing(false);
                        return;
                    }

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
