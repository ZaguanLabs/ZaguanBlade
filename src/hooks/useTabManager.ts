import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { EditorFacade, isTabsBackendAuthoritative } from '../services/editorFacade';
import { subscribeBladeNestedEventType } from '../services/bladeEvents';
import type { TabInfo } from '../types/blade';
import type { UncommittedChange } from '../types/uncommitted';

export interface Tab {
    id: string;
    title: string;
    type: 'file' | 'ephemeral';
    path?: string;
    content?: string;
    suggestedName?: string;
    highlightLines?: { startLine: number; endLine: number };
}

// Helper to convert backend TabInfo to frontend Tab
function tabInfoToTab(info: TabInfo): Tab {
    const isEphemeral = typeof info.tab_type === 'object' && info.tab_type.type === 'Ephemeral';
    return {
        id: info.id,
        title: info.title,
        type: isEphemeral ? 'ephemeral' : 'file',
        path: info.path ?? undefined,
        content: isEphemeral && 'data' in info.tab_type ? (info.tab_type as any).data.content : undefined,
        suggestedName: isEphemeral && 'data' in info.tab_type ? (info.tab_type as any).data.suggested_name : undefined,
    };
}

export interface AppBarTab {
    id: string;
    title: string;
    path?: string;
    isEphemeral: boolean;
    isDirty: boolean;
    hasVirtualChanges: boolean;
    isAiEdited: boolean;
    hasUnreadAiEdit: boolean;
}

export function useTabManager(uncommittedChanges: UncommittedChange[]) {
    const [activeTabId, setActiveTabId] = useState<string | null>(null);
    const [tabs, setTabs] = useState<Tab[]>([]);
    const [aiEditedFilePaths, setAiEditedFilePaths] = useState<Set<string>>(new Set());
    const [unseenAiEditedFilePaths, setUnseenAiEditedFilePaths] = useState<Set<string>>(new Set());
    const processingFilesRef = useRef<Set<string>>(new Set());

    // Tab history stack: tracks previously active tabs for "go back" on close
    const tabHistoryRef = useRef<string[]>([]);

    // Push to history whenever the active tab changes
    useEffect(() => {
        if (activeTabId) {
            const history = tabHistoryRef.current;
            if (history[history.length - 1] !== activeTabId) {
                history.push(activeTabId);
                if (history.length > 50) history.shift();
            }
        }
    }, [activeTabId]);

    // Pop the history stack to find the most recent tab that's still open
    const popTabHistory = useCallback((closedId: string, openTabs: Tab[]): string | null => {
        const history = tabHistoryRef.current;
        const openIds = new Set(openTabs.filter(t => t.id !== closedId).map(t => t.id));
        while (history.length > 0) {
            const prev = history.pop()!;
            if (prev !== closedId && openIds.has(prev)) {
                return prev;
            }
        }
        const remaining = openTabs.filter(t => t.id !== closedId);
        return remaining.length > 0 ? remaining[remaining.length - 1].id : null;
    }, []);

    // Listen for backend tab events when tabs_backend_authority is enabled
    useEffect(() => {
        const unsubscribeTabOpened = subscribeBladeNestedEventType('Editor', 'TabOpened', (payload) => {
            const newTab = tabInfoToTab(payload.tab);
            setTabs(prev => {
                if (prev.find(t => t.id === newTab.id)) return prev;
                return [...prev, newTab];
            });
        });
        const unsubscribeTabClosed = subscribeBladeNestedEventType('Editor', 'TabClosed', (payload) => {
            const closedId = payload.tab_id;
            setTabs(prev => {
                const remaining = prev.filter(t => t.id !== closedId);
                setActiveTabId(prevActive => {
                    if (prevActive !== closedId) return prevActive;
                    return popTabHistory(closedId, prev);
                });
                return remaining;
            });
        });
        const unsubscribeActiveTabChanged = subscribeBladeNestedEventType('Editor', 'ActiveTabChanged', (payload) => {
            setActiveTabId(payload.tab_id);
        });
        const unsubscribeTabsReordered = subscribeBladeNestedEventType('Editor', 'TabsReordered', (payload) => {
            const orderedIds = payload.tab_ids;
            setTabs(prev => {
                const tabMap = new Map(prev.map(t => [t.id, t]));
                return orderedIds.map(id => tabMap.get(id)).filter((t): t is Tab => !!t);
            });
        });
        const unsubscribeTabStateSnapshot = subscribeBladeNestedEventType('Editor', 'TabStateSnapshot', (payload) => {
            setTabs(payload.tabs.map(tabInfoToTab));
            setActiveTabId(payload.active_tab_id);
        });

        return () => {
            unsubscribeTabOpened();
            unsubscribeTabClosed();
            unsubscribeActiveTabChanged();
            unsubscribeTabsReordered();
            unsubscribeTabStateSnapshot();
        };
    }, [popTabHistory]);

    // Clear unseen AI edit flag when switching to a tab
    useEffect(() => {
        const activeTab = tabs.find(t => t.id === activeTabId);
        if (!activeTab || activeTab.type !== 'file' || !activeTab.path) return;

        setUnseenAiEditedFilePaths(prev => {
            if (!prev.has(activeTab.path!)) return prev;
            const next = new Set(prev);
            next.delete(activeTab.path!);
            return next;
        });
    }, [activeTabId, tabs]);

    const hasPendingVirtualChanges = useCallback((path?: string) => {
        if (!path) return false;
        return uncommittedChanges.some(change =>
            change.file_path === path
            || change.file_path.endsWith(path)
            || path.endsWith(change.file_path)
        );
    }, [uncommittedChanges]);

    // Memoize uncommitted changes count and paths to avoid frequent recomputation
    const uncommittedChangesMetadata = useMemo(() => ({
        count: uncommittedChanges.length,
        paths: new Set(uncommittedChanges.map(c => c.file_path)),
    }), [uncommittedChanges]);

    const handleTabClick = useCallback((tabId: string) => {
        setActiveTabId(tabId);
    }, []);

    const handleFileSelect = useCallback((path: string, highlightLines?: { startLine: number; endLine: number } | null) => {
        const existingTab = tabs.find(t => t.type === 'file' && t.path === path);
        if (!existingTab) {
            const filename = path.split('/').pop() || path;
            const tabId = `file-${path}`;

            if (isTabsBackendAuthoritative()) {
                EditorFacade.openTab(tabId, filename, path, 'file').catch(console.error);
                EditorFacade.setActiveTab(tabId).catch(console.error);
            } else {
                const newTab: Tab = {
                    id: tabId,
                    title: filename,
                    type: 'file',
                    path,
                    highlightLines: highlightLines ?? undefined,
                };
                setTabs(prev => [...prev, newTab]);
                setActiveTabId(tabId);
            }
        } else {
            if (highlightLines) {
                setTabs(prev => prev.map(tab =>
                    tab.id === existingTab.id
                        ? { ...tab, highlightLines }
                        : tab
                ));
            }
            if (isTabsBackendAuthoritative()) {
                EditorFacade.setActiveTab(existingTab.id).catch(console.error);
            } else {
                setActiveTabId(existingTab.id);
            }
        }
    }, [tabs]);

    const handleTabClose = useCallback((tabId: string) => {
        if (isTabsBackendAuthoritative()) {
            EditorFacade.closeTab(tabId).catch(console.error);
        } else {
            setActiveTabId(prev => {
                if (prev === tabId) return popTabHistory(tabId, tabs);
                return prev;
            });
            setTabs(prev => prev.filter(t => t.id !== tabId));
        }
    }, [tabs, popTabHistory]);

    const handleTabReorder = useCallback((fromIndex: number, toIndex: number) => {
        setTabs(prev => {
            const newTabs = [...prev];
            const [movedTab] = newTabs.splice(fromIndex, 1);
            if (!movedTab) {
                return prev;
            }
            newTabs.splice(toIndex, 0, movedTab);
            if (isTabsBackendAuthoritative()) {
                EditorFacade.reorderTabs(newTabs.map(tab => tab.id)).catch(console.error);
            }
            return newTabs;
        });
    }, []);

    const handleTabMoveToBeginning = useCallback((tabId: string) => {
        setTabs(prev => {
            const index = prev.findIndex(tab => tab.id === tabId);
            if (index <= 0) {
                return prev;
            }
            const next = [...prev];
            const [moved] = next.splice(index, 1);
            next.unshift(moved);
            if (isTabsBackendAuthoritative()) {
                EditorFacade.reorderTabs(next.map(tab => tab.id)).catch(console.error);
            }
            return next;
        });
    }, []);

    const handleTabMoveToEnd = useCallback((tabId: string) => {
        setTabs(prev => {
            const index = prev.findIndex(tab => tab.id === tabId);
            if (index === -1 || index === prev.length - 1) {
                return prev;
            }
            const next = [...prev];
            const [moved] = next.splice(index, 1);
            next.push(moved);
            if (isTabsBackendAuthoritative()) {
                EditorFacade.reorderTabs(next.map(tab => tab.id)).catch(console.error);
            }
            return next;
        });
    }, []);

    const handleTabCloseAll = useCallback(() => {
        if (isTabsBackendAuthoritative()) {
            tabs.forEach(tab => {
                EditorFacade.closeTab(tab.id).catch(console.error);
            });
            return;
        }
        setTabs([]);
        setActiveTabId(null);
    }, [tabs]);

    const handleTabCloseOthers = useCallback((tabId: string) => {
        if (isTabsBackendAuthoritative()) {
            tabs.filter(tab => tab.id !== tabId).forEach(tab => {
                EditorFacade.closeTab(tab.id).catch(console.error);
            });
            EditorFacade.setActiveTab(tabId).catch(console.error);
            return;
        }
        setTabs(prev => prev.filter(tab => tab.id === tabId));
        setActiveTabId(tabId);
    }, [tabs]);

    const handleEphemeralSave = useCallback(async (ephemeralTabId: string, savedPath: string) => {
        console.debug('[TabManager] handleEphemeralSave called:', { ephemeralTabId, savedPath });

        setTabs(prev => {
            const ephemeralTab = prev.find(t => t.id === ephemeralTabId);
            if (!ephemeralTab) {
                console.debug('[TabManager] Ephemeral tab not found:', ephemeralTabId);
                return prev;
            }

            console.debug('[TabManager] Found ephemeral tab:', ephemeralTab);
            const filename = savedPath.split('/').pop() || savedPath;
            const newTab: Tab = {
                id: `file-${savedPath}`,
                title: filename,
                type: 'file',
                path: savedPath,
            };

            console.debug('[TabManager] Creating new file tab:', newTab);
            return [...prev.filter(t => t.id !== ephemeralTabId), newTab];
        });

        const newTabId = `file-${savedPath}`;
        console.debug('[TabManager] Switching to new tab:', newTabId);
        setActiveTabId(newTabId);

        try {
            console.debug('[TabManager] Calling open_file_in_editor:', savedPath);
            await invoke('open_file_in_editor', { path: savedPath });
            console.debug('[TabManager] open_file_in_editor completed successfully');
        } catch (error) {
            console.error('[TabManager] Failed to open saved file:', error);
        }
    }, []);

    // Keyboard shortcuts for tab management
    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            // Ctrl-W to close current tab
            if (e.ctrlKey && e.key === 'w') {
                e.preventDefault();
                if (activeTabId) {
                    handleTabClose(activeTabId);
                }
            }

            // F12 to toggle DevTools
            if (e.key === 'F12') {
                e.preventDefault();
                invoke('toggle_devtools').catch(err => console.error('Failed to toggle devtools:', err));
                return;
            }

            // Ctrl-Tab to cycle right through tabs
            if (e.ctrlKey && e.key === 'Tab') {
                e.preventDefault();
                if (tabs.length > 1 && activeTabId) {
                    const currentIndex = tabs.findIndex(t => t.id === activeTabId);
                    if (e.shiftKey) {
                        const prevIndex = (currentIndex - 1 + tabs.length) % tabs.length;
                        setActiveTabId(tabs[prevIndex].id);
                    } else {
                        const nextIndex = (currentIndex + 1) % tabs.length;
                        setActiveTabId(tabs[nextIndex].id);
                    }
                }
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [activeTabId, tabs, handleTabClose]);

    const activeTab = useMemo(() => tabs.find(t => t.id === activeTabId) ?? null, [tabs, activeTabId]);
    const activeFilename = useMemo(() => {
        if (!activeTab) return null;
        if (activeTab.path) {
            const parts = activeTab.path.split(/[/\\]/).filter(Boolean);
            return parts.length > 0 ? parts[parts.length - 1] : activeTab.title;
        }
        return activeTab.title;
    }, [activeTab]);

    return {
        tabs,
        setTabs,
        activeTabId,
        setActiveTabId,
        activeTab,
        activeFilename,
        aiEditedFilePaths,
        setAiEditedFilePaths,
        unseenAiEditedFilePaths,
        setUnseenAiEditedFilePaths,
        processingFilesRef,
        handleTabClick,
        handleFileSelect,
        handleTabClose,
        handleTabReorder,
        handleTabMoveToBeginning,
        handleTabMoveToEnd,
        handleTabCloseAll,
        handleTabCloseOthers,
        handleEphemeralSave,
        popTabHistory,
    };
}
