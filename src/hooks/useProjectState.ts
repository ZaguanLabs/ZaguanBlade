import { useEffect, useRef, useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

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
    const hasLoadedRef = useRef(false);
    const [loaded, setLoaded] = useState(false);
    const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    const saveState = useCallback(async () => {
        if (!projectPath) return;

        const activeTab = tabs.find(t => t.id === activeTabId);
        const activeFile = activeTab?.type === 'file' ? activeTab.path || null : null;

        const state: ProjectState = {
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
            chat_panel_width: null,
            explorer_width: null,
        };

        try {
            await invoke('save_project_state', { stateData: state });
            console.log('[ProjectState] Saved state for:', projectPath);
        } catch (e) {
            console.error('[ProjectState] Failed to save state:', e);
        }
    }, [projectPath, tabs, activeTabId, selectedModelId, terminals, activeTerminalId, terminalHeight]);

    const loadState = useCallback(async (): Promise<ProjectState | null> => {
        if (!projectPath) return null;

        try {
            const state = await invoke<ProjectState | null>('load_project_state', { projectPath });
            if (state) {
                console.log('[ProjectState] Loaded state for:', projectPath, state);
            }
            return state;
        } catch (e) {
            console.error('[ProjectState] Failed to load state:', e);
            return null;
        }
    }, [projectPath]);

    // Load state on mount when project path is available
    useEffect(() => {
        if (!projectPath || hasLoadedRef.current) return;

        const load = async () => {
            const state = await loadState();
            
            // IMPORTANT: Call onStateLoaded BEFORE setting loaded to true
            // This ensures the model is set before useWarmup sees ready=true
            if (state && onStateLoaded) {
                console.log('[ProjectState] Restoring saved state');
                onStateLoaded(state);
            } else {
                console.log('[ProjectState] No saved state found, will create on next save');
            }
            
            // Mark as loaded AFTER state is restored
            // This ensures useWarmup sees the correct model when it becomes ready
            hasLoadedRef.current = true;
            setLoaded(true);
        };

        load();
    }, [projectPath, loadState, onStateLoaded]);

    // Debounced auto-save on state changes
    useEffect(() => {
        if (!projectPath || !hasLoadedRef.current) return;

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
    }, [projectPath, tabs, activeTabId, selectedModelId, terminals, activeTerminalId, terminalHeight, saveState]);

    // Save on window unload
    useEffect(() => {
        const handleBeforeUnload = () => {
            if (projectPath && hasLoadedRef.current) {
                // Use synchronous approach for beforeunload
                // Note: invoke is async, so this may not complete
                // Consider using navigator.sendBeacon in the future
                saveState();
            }
        };

        window.addEventListener('beforeunload', handleBeforeUnload);
        return () => {
            window.removeEventListener('beforeunload', handleBeforeUnload);
        };
    }, [projectPath, saveState]);

    return { saveState, loadState, loaded };
}
