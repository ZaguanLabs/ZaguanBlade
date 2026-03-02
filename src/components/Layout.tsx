import React, { useState, useEffect, useRef, useCallback, useMemo, Suspense } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ExplorerPanel } from './ExplorerPanel';
import { EditorPanel } from './EditorPanel';
import { TerminalPane, TerminalPaneHandle } from './TerminalPane';
import { DocumentViewer } from './DocumentViewer';
import { AppBar } from './AppBar';
import { GitBranch, Settings, Clock } from 'lucide-react';
import { EditorProvider, useEditorActions } from '../contexts/EditorContext';
import { useUncommittedChanges } from '../hooks/useUncommittedChanges';
import { useChat } from '../hooks/useChat';
import { StorageSetupModal } from './StorageSetupModal';
import { useProjectState, type ProjectState } from '../hooks/useProjectState';
import { useWarmup } from '../hooks/useWarmup';
import { useGitStatus } from '../hooks/useGitStatus';
import { EditorFacade, isTabsBackendAuthoritative } from '../services/editorFacade';
import type { BladeEventEnvelope, EditorEvent, TabInfo } from '../types/blade';
const ChatPanel = React.lazy(() => import('./ChatPanel').then(module => ({ default: module.ChatPanel })));
const GitPanel = React.lazy(() => import('./GitPanel').then(module => ({ default: module.GitPanel })));
const FileHistoryPanel = React.lazy(() => import('./FileHistoryPanel').then(module => ({ default: module.FileHistoryPanel })));
const SettingsModal = React.lazy(() => import('./SettingsModal').then(module => ({ default: module.SettingsModal })));
const ProtocolExplorer = React.lazy(() => import('./dev/ProtocolExplorer').then(module => ({ default: module.ProtocolExplorer })));
import type { BackendSettings } from '../types/settings';

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

interface Tab {
    id: string;
    title: string;
    type: 'file' | 'ephemeral';
    path?: string;
    content?: string;
    suggestedName?: string;
    highlightLines?: { startLine: number; endLine: number };
}

const AppLayoutInner: React.FC = () => {
    const { t } = useTranslation();
    const appWindow = getCurrentWindow();
    const [activeTabId, setActiveTabId] = useState<string | null>(null);
    const [tabs, setTabs] = useState<Tab[]>([]);
    const [aiEditedFilePaths, setAiEditedFilePaths] = useState<Set<string>>(new Set());
    const [unseenAiEditedFilePaths, setUnseenAiEditedFilePaths] = useState<Set<string>>(new Set());
    const [terminalHeight, setTerminalHeight] = useState(300);
    const [chatPanelWidth, setChatPanelWidth] = useState(400);
    const [isChatDragging, setIsChatDragging] = useState(false);
    const [isTerminalDragging, setIsTerminalDragging] = useState(false);


    // Sidebar State
    const [isSidebarOpen, setIsSidebarOpen] = useState(false);
    const [activeSidebar, setActiveSidebar] = useState<'explorer' | 'git' | 'history'>('explorer');

    const chat = useChat();
    const [wasStoppedByUser, setWasStoppedByUser] = useState(false);
    const { changes: uncommittedChanges, acceptAll: acceptAllChanges, rejectAll: rejectAllChanges } = useUncommittedChanges();
    const {
        status: gitStatus,
        files: gitFiles,
        error: gitError,
        filesError: gitFilesError,
        lastRefreshedAt: gitLastRefreshedAt,
        refresh: refreshGitStatus,
        stageFile: stageGitFile,
        unstageFile: unstageGitFile,
        stageAll: stageAllGit,
        unstageAll: unstageAllGit,
        commit: commitGit,
        push: pushGit,
        diff: diffGit,
        generateCommitMessage: generateGitCommitMessage,
        commitPreflight: commitPreflightGit,
    } = useGitStatus();
    const gitChangedCount = gitStatus?.changedCount ?? 0;
    const { selectedModelId, setSelectedModelId, messages, refreshModels } = chat;
    const processingFilesRef = useRef<Set<string>>(new Set());
    const terminalPaneRef = useRef<TerminalPaneHandle>(null);

    const handleStopGeneration = useCallback(async () => {
        setWasStoppedByUser(true);
        await chat.stopGeneration();
    }, [chat.stopGeneration]);

    // Tab history stack: tracks previously active tabs for "go back" on close
    const tabHistoryRef = useRef<string[]>([]);

    // Push to history whenever the active tab changes
    useEffect(() => {
        if (activeTabId) {
            const history = tabHistoryRef.current;
            // Don't push duplicates at the top
            if (history[history.length - 1] !== activeTabId) {
                history.push(activeTabId);
                // Cap at 50 entries to avoid unbounded growth
                if (history.length > 50) history.shift();
            }
        }
    }, [activeTabId]);

    // Pop the history stack to find the most recent tab that's still open
    const popTabHistory = (closedId: string, openTabs: Tab[]): string | null => {
        const history = tabHistoryRef.current;
        const openIds = new Set(openTabs.filter(t => t.id !== closedId).map(t => t.id));
        while (history.length > 0) {
            const prev = history.pop()!;
            if (prev !== closedId && openIds.has(prev)) {
                return prev;
            }
        }
        // Fallback: pick the last remaining tab, or null
        const remaining = openTabs.filter(t => t.id !== closedId);
        return remaining.length > 0 ? remaining[remaining.length - 1].id : null;
    };

    // Sync active tab and open file paths to EditorContext
    const { setActiveFile, setOpenFiles } = useEditorActions();
    useEffect(() => {
        const activeTab = tabs.find(t => t.id === activeTabId);
        setActiveFile(activeTab?.path || null);
    }, [activeTabId, tabs, setActiveFile]);

    useEffect(() => {
        const filePaths = tabs
            .filter(t => t.type === 'file' && t.path)
            .map(t => t.path!);
        setOpenFiles(filePaths);
    }, [tabs, setOpenFiles]);

    // Listen for backend tab events when tabs_backend_authority is enabled
    useEffect(() => {
        let unlisten: (() => void) | undefined;

        const setup = async () => {
            unlisten = await listen<BladeEventEnvelope>('blade-event', (event) => {
                const bladeEvent = event.payload.event;
                if (bladeEvent.type !== 'Editor') return;

                const editorEvent = bladeEvent.payload as EditorEvent;

                if (editorEvent.type === 'TabOpened') {
                    const newTab = tabInfoToTab(editorEvent.payload.tab);
                    setTabs(prev => {
                        if (prev.find(t => t.id === newTab.id)) return prev;
                        return [...prev, newTab];
                    });
                } else if (editorEvent.type === 'TabClosed') {
                    const closedId = editorEvent.payload.tab_id;
                    setTabs(prev => {
                        const remaining = prev.filter(t => t.id !== closedId);
                        setActiveTabId(prevActive => {
                            if (prevActive !== closedId) return prevActive;
                            return popTabHistory(closedId, prev);
                        });
                        return remaining;
                    });
                } else if (editorEvent.type === 'ActiveTabChanged') {
                    setActiveTabId(editorEvent.payload.tab_id);
                } else if (editorEvent.type === 'TabsReordered') {
                    const orderedIds = editorEvent.payload.tab_ids;
                    setTabs(prev => {
                        const tabMap = new Map(prev.map(t => [t.id, t]));
                        return orderedIds.map(id => tabMap.get(id)).filter((t): t is Tab => !!t);
                    });
                } else if (editorEvent.type === 'TabStateSnapshot') {
                    const { tabs: backendTabs, active_tab_id } = editorEvent.payload;
                    setTabs(backendTabs.map(tabInfoToTab));
                    setActiveTabId(active_tab_id);
                }
            });
        };

        setup().catch(console.error);

        return () => {
            if (unlisten) unlisten();
        };
    }, []);

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

    const appBarTabs = useMemo(() => tabs.map(t => {
        const isFileTab = t.type === 'file' && !!t.path;
        const path = t.path;

        return {
            id: t.id,
            title: t.title,
            isEphemeral: t.type === 'ephemeral',
            isDirty: false,
            hasVirtualChanges: isFileTab ? hasPendingVirtualChanges(path) : false,
            isAiEdited: isFileTab ? aiEditedFilePaths.has(path!) : false,
            hasUnreadAiEdit: isFileTab ? unseenAiEditedFilePaths.has(path!) : false,
        };
    }), [tabs, hasPendingVirtualChanges, aiEditedFilePaths, unseenAiEditedFilePaths]);

    const handleTabClick = useCallback((tabId: string) => {
        setActiveTabId(tabId);
    }, []);

    // Research progress state
    const [researchProgress, setResearchProgress] = useState<{
        message: string;
        stage: string;
        percent: number;
        isActive: boolean;
    } | null>(null);

    // Settings modal state
    const [isSettingsOpen, setIsSettingsOpen] = useState(false);

    // Listen for open-settings custom event (from WelcomePage or ChatPanel)
    useEffect(() => {
        const handleOpenSettings = () => setIsSettingsOpen(true);
        document.addEventListener('open-settings', handleOpenSettings);
        return () => document.removeEventListener('open-settings', handleOpenSettings);
    }, []);

    // First-time setup modal state (RFC-002)
    const [showStorageSetup, setShowStorageSetup] = useState(false);
    const [hasCheckedZblade, setHasCheckedZblade] = useState(false);

    const [workspacePath, setWorkspacePath] = useState<string | null>(null);
    const [userId, setUserId] = useState<string | null>(null);
    const [projectId, setProjectId] = useState<string | null>(null);

    const projectName = useMemo(() => {
        if (!workspacePath) return null;
        const normalized = workspacePath.replace(/[/\\]+$/, '');
        const segments = normalized.split(/[/\\]/).filter(Boolean);
        return segments.length > 0 ? segments[segments.length - 1] : null;
    }, [workspacePath]);

    const activeTab = useMemo(() => tabs.find(t => t.id === activeTabId) ?? null, [tabs, activeTabId]);
    const activeFilename = useMemo(() => {
        if (!activeTab) return null;
        if (activeTab.path) {
            const parts = activeTab.path.split(/[/\\]/).filter(Boolean);
            return parts.length > 0 ? parts[parts.length - 1] : activeTab.title;
        }
        return activeTab.title;
    }, [activeTab]);

    useEffect(() => {
        const titleParts = ['Zaguán Blade'];
        if (projectName) titleParts.push(projectName);
        if (activeFilename) titleParts.push(activeFilename);
        appWindow.setTitle(titleParts.join(' - ')).catch((err) => {
            console.error('[Layout] Failed to set window title:', err);
        });
    }, [appWindow, projectName, activeFilename]);

    // Trigger backend to load project settings on mount and when settings change
    useEffect(() => {
        if (!workspacePath) return;

        const loadSettings = async () => {
            try {
                await invoke<BackendSettings>('load_project_settings', {
                    projectPath: workspacePath,
                });
            } catch (e) {
                console.error('[Layout] Failed to load project settings:', e);
            }
        };

        loadSettings();

        // Listen for settings changes from SettingsModal
        const unlistenPromise = listen('global-settings-changed', loadSettings);

        return () => {
            unlistenPromise.then(unlisten => unlisten());
        };
    }, [workspacePath]);

    // Fetch current workspace and user_id on mount
    const initializedRef = useRef(false);
    useEffect(() => {
        if (initializedRef.current) return;
        initializedRef.current = true;

        const fetchWorkspace = async () => {
            try {
                const path = await invoke<string | null>('get_current_workspace');
                setWorkspacePath(path);

                // Fetch project_id for this workspace
                if (path) {
                    try {
                        const id = await invoke<string | null>('get_project_id', { workspacePath: path });
                        setProjectId(id);
                    } catch (e) {
                        console.error('[Layout] Failed to get project_id:', e);
                    }
                }
            } catch (e) {
                console.error('[Layout] Failed to get workspace:', e);
            }
        };
        const fetchUserId = async () => {
            try {
                const id = await invoke<string | null>('get_user_id');
                if (id) {
                    setUserId(id);
                }
            } catch (e) {
                console.error('[Layout] Failed to get user_id:', e);
            }
        };
        fetchWorkspace();
        fetchUserId();
    }, []);

    // RFC-002: Check if .zblade directory exists for first-time setup
    useEffect(() => {
        const checkZbladeDir = async () => {
            if (!workspacePath || hasCheckedZblade) return;

            try {
                const exists = await invoke<boolean>('has_zblade_directory', { projectPath: workspacePath });
                setHasCheckedZblade(true);
                if (!exists) {
                    setShowStorageSetup(true);
                }
            } catch (e) {
                console.error('[Layout] Failed to check .zblade directory:', e);
                setHasCheckedZblade(true);
            }
        };

        checkZbladeDir();
    }, [workspacePath, hasCheckedZblade]);

    // Handle project state restoration
    const handleStateLoaded = useCallback((state: ProjectState) => {
        console.debug('[Layout] Restoring project state:', state);

        // Restore tabs
        if (state.open_tabs && state.open_tabs.length > 0) {
            const restoredTabs: Tab[] = state.open_tabs.map(t => ({
                id: t.id,
                title: t.title,
                type: t.type as 'file' | 'ephemeral',
                path: t.path,
            }));
            setTabs(restoredTabs);

            // Restore active tab
            if (state.active_file) {
                const activeTab = restoredTabs.find(t => t.path === state.active_file);
                if (activeTab) {
                    setActiveTabId(activeTab.id);
                }
            } else if (restoredTabs.length > 0) {
                setActiveTabId(restoredTabs[0].id);
            }
        }

        // Restore terminal height
        if (state.terminal_height) {
            setTerminalHeight(state.terminal_height);
        }

        // Restore chat panel width
        if (state.chat_panel_width) {
            setChatPanelWidth(state.chat_panel_width);
        }

        // Restore selected model
        if (state.selected_model_id) {
            setSelectedModelId(state.selected_model_id);
        }

        // Restore terminals via ref
        if (state.terminals && state.terminals.length > 0 && terminalPaneRef.current) {
            terminalPaneRef.current.restoreTerminals(
                state.terminals,
                state.active_terminal_id || undefined
            );
        }

        // Project state load can restore pending uncommitted AI changes in backend state.
        // Notify all hook instances to refresh and show accept/reject prompts after startup.
        window.dispatchEvent(new CustomEvent('uncommitted-changes-updated'));
    }, [setSelectedModelId]);

    // Get terminal state for persistence
    const getTerminalState = useCallback(() => {
        if (terminalPaneRef.current) {
            return terminalPaneRef.current.getTerminalState();
        }
        return { terminals: [], activeId: 'term-1' };
    }, []);

    const terminalState = getTerminalState();

    // Project state persistence
    const { loaded: stateLoaded, isClosing } = useProjectState({
        projectPath: workspacePath,
        tabs: tabs.map(t => ({ id: t.id, title: t.title, type: t.type, path: t.path })),
        activeTabId,
        selectedModelId,
        terminals: terminalState.terminals,
        activeTerminalId: terminalState.activeId,
        terminalHeight,
        chatPanelWidth,
        onStateLoaded: handleStateLoaded,
    });

    // Cache warmup (Blade Protocol v2.1)
    // Automatically warms cache on launch, model change, and workspace change
    // Wait for stateLoaded to prevent multiple warmups during initialization
    const { trackActivity } = useWarmup(workspacePath, selectedModelId, stateLoaded);



    // Auto-open files when edit proposals arrive (without stealing focus)
    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;

        const unlistenPromise = listen<{ id: string; path: string; old_content: string; new_content: string }[]>('propose-edit', (event) => {
            if (event.payload.length === 0) return;

            const editedPaths = event.payload.map(edit => edit.path);

            setAiEditedFilePaths(prev => {
                const next = new Set(prev);
                for (const path of editedPaths) next.add(path);
                return next;
            });

            setUnseenAiEditedFilePaths(prev => {
                const next = new Set(prev);
                for (const path of editedPaths) next.add(path);
                return next;
            });

            setTabs(prev => {
                let next = prev;

                for (const path of editedPaths) {
                    const existingTab = next.find(t => t.type === 'file' && t.path === path);
                    if (existingTab) continue;

                    const filename = path.split('/').pop() || path;
                    const newTab: Tab = {
                        id: `file-${path}`,
                        title: filename,
                        type: 'file',
                        path,
                    };

                    if (next === prev) {
                        next = [...prev, newTab];
                    } else {
                        next.push(newTab);
                    }
                }

                return next;
            });
        });

        return () => {
            unlistenPromise
                .then(unlisten => unlisten())
                .catch(console.error);
        };
    }, []);

    const handleFileSelect = (path: string) => {
        // Add to tabs if not already open
        const existingTab = tabs.find(t => t.type === 'file' && t.path === path);
        if (!existingTab) {
            const filename = path.split('/').pop() || path;
            const tabId = `file-${path}`;

            // If backend authority, dispatch to backend (it will emit TabOpened event)
            if (isTabsBackendAuthoritative()) {
                EditorFacade.openTab(tabId, filename, path, 'file').catch(console.error);
                EditorFacade.setActiveTab(tabId).catch(console.error);
            } else {
                // Legacy: update local state directly
                const newTab: Tab = {
                    id: tabId,
                    title: filename,
                    type: 'file',
                    path,
                };
                setTabs(prev => [...prev, newTab]);
                setActiveTabId(tabId);
            }
        } else {
            // Tab exists, just activate it
            if (isTabsBackendAuthoritative()) {
                EditorFacade.setActiveTab(existingTab.id).catch(console.error);
            } else {
                setActiveTabId(existingTab.id);
            }
        }
    };

    const handleTabClose = (tabId: string) => {
        if (isTabsBackendAuthoritative()) {
            EditorFacade.closeTab(tabId).catch(console.error);
        } else {
            if (activeTabId === tabId) {
                setActiveTabId(popTabHistory(tabId, tabs));
            }
            setTabs(prev => prev.filter(t => t.id !== tabId));
        }
    };

    const handleEphemeralSave = async (ephemeralTabId: string, savedPath: string) => {
        console.debug('[Layout] handleEphemeralSave called:', { ephemeralTabId, savedPath });
        
        // Convert ephemeral tab to regular file tab
        setTabs(prev => {
            const ephemeralTab = prev.find(t => t.id === ephemeralTabId);
            if (!ephemeralTab) {
                console.debug('[Layout] Ephemeral tab not found:', ephemeralTabId);
                return prev;
            }

            console.debug('[Layout] Found ephemeral tab:', ephemeralTab);
            const filename = savedPath.split('/').pop() || savedPath;
            const newTab: Tab = {
                id: `file-${savedPath}`,
                title: filename,
                type: 'file',
                path: savedPath,
            };

            console.debug('[Layout] Creating new file tab:', newTab);
            // Remove ephemeral tab and add file tab
            return [...prev.filter(t => t.id !== ephemeralTabId), newTab];
        });

        // Switch to the new file tab
        const newTabId = `file-${savedPath}`;
        console.debug('[Layout] Switching to new tab:', newTabId);
        setActiveTabId(newTabId);

        // Trigger backend to open the file so it loads in the editor
        try {
            console.debug('[Layout] Calling open_file_in_editor:', savedPath);
            await invoke('open_file_in_editor', { path: savedPath });
            console.debug('[Layout] open_file_in_editor completed successfully');
        } catch (error) {
            console.error('[Layout] Failed to open saved file:', error);
        }
    };

    // Terminal panel resize handler
    const handleTerminalMouseDown = (e: React.MouseEvent) => {
        if (e.button !== 0) return;
        setIsTerminalDragging(true);
        e.preventDefault();
    };

    useEffect(() => {
        const handleTerminalMouseMove = (e: MouseEvent) => {
            if (!isTerminalDragging) return;
            const col = editorColumnRef.current;
            if (!col) return;
            const rect = col.getBoundingClientRect();
            const newHeight = rect.bottom - e.clientY;
            if (newHeight >= 80 && newHeight <= rect.height - 60) {
                setTerminalHeight(newHeight);
            }
        };

        const handleTerminalMouseUp = () => {
            setIsTerminalDragging(false);
        };

        if (isTerminalDragging) {
            document.addEventListener('mousemove', handleTerminalMouseMove);
            document.addEventListener('mouseup', handleTerminalMouseUp);
            document.body.classList.add('resize-y-cursor');
        }

        return () => {
            document.removeEventListener('mousemove', handleTerminalMouseMove);
            document.removeEventListener('mouseup', handleTerminalMouseUp);
            document.body.classList.remove('resize-y-cursor');
        };
    }, [isTerminalDragging]);

    // Chat panel resize handler
    const handleChatMouseDown = (e: React.MouseEvent) => {
        if (e.button !== 0) return;
        setIsChatDragging(true);
        e.preventDefault();
    };

    useEffect(() => {
        const handleChatMouseMove = (e: MouseEvent) => {
            if (!isChatDragging) return;
            // Calculate new width from right edge
            const newWidth = window.innerWidth - e.clientX;
            // Clamp width between 280 and 800
            if (newWidth >= 280 && newWidth <= 800) {
                setChatPanelWidth(newWidth);
            }
        };

        const handleChatMouseUp = () => {
            setIsChatDragging(false);
        };

        if (isChatDragging) {
            document.addEventListener('mousemove', handleChatMouseMove);
            document.addEventListener('mouseup', handleChatMouseUp);
            document.body.classList.add('resize-x-cursor');
        }

        return () => {
            document.removeEventListener('mousemove', handleChatMouseMove);
            document.removeEventListener('mouseup', handleChatMouseUp);
            document.body.classList.remove('resize-x-cursor');
        };
    }, [isChatDragging]);

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
                        // Ctrl-Shift-Tab: cycle left
                        const prevIndex = (currentIndex - 1 + tabs.length) % tabs.length;
                        setActiveTabId(tabs[prevIndex].id);
                    } else {
                        // Ctrl-Tab: cycle right
                        const nextIndex = (currentIndex + 1) % tabs.length;
                        setActiveTabId(tabs[nextIndex].id);
                    }
                }
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [activeTabId, tabs]);

    // Listen for open-file and open-ephemeral-document events
    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
        const unlistenPromises: Promise<() => void>[] = [];
            const handleOpenFile = (path: string, sourceEvent: string) => {
                console.debug(`Opening file from backend (${sourceEvent}):`, path);
                const tabId = `file-${path}`;

                // Prevent duplicate processing
                if (processingFilesRef.current.has(path)) {
                    console.debug('[LAYOUT] Ignoring duplicate file open event for:', path);
                    return;
                }
                processingFilesRef.current.add(path);

                setTabs(prev => {
                    const existingTab = prev.find(t => t.type === 'file' && t.path === path);
                    if (existingTab) {
                        processingFilesRef.current.delete(path);
                        return prev;
                    }
                    const filename = path.split('/').pop() || path;
                    const newTab: Tab = {
                        id: tabId,
                        title: filename,
                        type: 'file',
                        path,
                    };
                    processingFilesRef.current.delete(path);
                    return [...prev, newTab];
                });
                // Keep current focus while allowing backend/AI-opened files to appear in background tabs.
                setActiveTabId(prev => prev ?? tabId);
            };

            // Current backend event name (Rust emits this)
            unlistenPromises.push(listen<string>('open-file', (event) => {
                handleOpenFile(event.payload, 'open-file');
            }));

            // Backwards-compatible alias (kept for older emitters)
            unlistenPromises.push(listen<string>('file-opened', (event) => {
                handleOpenFile(event.payload, 'file-opened');
            }));

            unlistenPromises.push(listen<{ path: string; start_line: number; end_line: number }>('open-file-with-highlight', (event) => {
                console.debug('Opening file with highlight from backend:', event.payload);
                const { path, start_line, end_line } = event.payload;
                const tabId = `file-${path}`;
                setTabs(prev => {
                    const existingTab = prev.find(t => t.type === 'file' && t.path === path);
                    if (existingTab) {
                        return prev.map(t =>
                            t.id === existingTab.id
                                ? { ...t, highlightLines: { startLine: start_line, endLine: end_line } }
                                : t
                        );
                    }
                    const filename = path.split('/').pop() || path;
                    const newTab: Tab = {
                        id: tabId,
                        title: filename,
                        type: 'file',
                        path,
                        highlightLines: { startLine: start_line, endLine: end_line },
                    };
                    return [...prev, newTab];
                });
                // Do not steal editor focus; only activate if no file is currently active.
                setActiveTabId(prev => prev ?? tabId);
            }));

            unlistenPromises.push(listen<{ id: string; title: string; content: string; suggestedName: string }>('open-ephemeral-document', (event) => {
                console.debug('[LAYOUT] 📥 Received open-ephemeral-document event:', {
                    id: event.payload.id,
                    title: event.payload.title,
                    contentLength: event.payload.content.length,
                    suggestedName: event.payload.suggestedName
                });

                // Clear research progress when result arrives
                setResearchProgress(null);

                const { id, title, content, suggestedName } = event.payload;

                setTabs(prev => {
                    // Check if tab already exists
                    const existingTab = prev.find(t => t.id === id);
                    if (existingTab) {
                        console.debug('[LAYOUT] ⚠️ Tab already exists, just activating:', id);
                        return prev;
                    }

                    console.debug('[LAYOUT] ✅ Creating new tab with ID:', id);
                    const newTab: Tab = {
                        id,
                        title,
                        type: 'ephemeral',
                        content,
                        suggestedName,
                    };
                    console.debug('[LAYOUT] Adding tab to existing tabs:', prev.length, '→', prev.length + 1);
                    return [...prev, newTab];
                });
                setActiveTabId(id);
            }));

            // Listen for research progress events
            unlistenPromises.push(listen<{ message: string; stage: string; percent: number }>('research-progress', (event) => {
                console.debug('[LAYOUT] Research progress:', event.payload);
                
                // Set temporary state for active indicator
                setResearchProgress({
                    ...event.payload,
                    isActive: true
                });

                // Persist research activity in message history
                chat.setConversation(prev => {
                    const updated = [...prev];
                    // Find the last assistant message to attach research activity
                    for (let i = updated.length - 1; i >= 0; i--) {
                        if (updated[i].role === 'Assistant') {
                            const msg = updated[i];
                            const activityId = crypto.randomUUID();
                            const newActivity = {
                                id: activityId,
                                message: event.payload.message,
                                stage: event.payload.stage,
                                percent: event.payload.percent,
                                timestamp: Date.now(),
                            };

                            // Add or update research activity
                            const existingActivities = msg.researchActivities || [];
                            const newActivities = [...existingActivities, newActivity];
                            
                            // Update blocks to include research_progress block
                            const newBlocks = [...(msg.blocks || [])];
                            if (!newBlocks.some(b => b.type === 'research_progress' && b.id === activityId)) {
                                newBlocks.push({ type: 'research_progress', id: activityId });
                            }

                            updated[i] = {
                                ...msg,
                                researchActivities: newActivities,
                                blocks: newBlocks
                            };
                            break;
                        }
                    }
                    return updated;
                });
            }));

            // Listen for chat errors to clear progress
            unlistenPromises.push(listen('chat-error', () => {
                setResearchProgress(null);
            }));

            // NOTE: context-length-exceeded is now handled in useChat.ts where it belongs

            // Listen for change-applied events to convert ephemeral tabs to file tabs
            unlistenPromises.push(listen<{ change_id: string; file_path: string }>('change-applied', (event) => {
                console.debug('[LAYOUT] Change applied:', event.payload);
                const { change_id, file_path } = event.payload;

                // Find any ephemeral tab that might be associated with this change
                // 1. Check for explicit "new-file-toolId" tabs
                // 2. Check for generic ephemeral tabs that match the filename
                const filename = file_path.split('/').pop() || file_path;

                // Mark this file as being processed
                processingFilesRef.current.add(file_path);

                setTabs(prev => {
                    const ephemeralTab = prev.find(t =>
                        t.id === `new-file-${change_id}` ||
                        (t.type === 'ephemeral' && (
                            t.suggestedName === filename ||
                            t.title === filename ||
                            t.suggestedName?.includes(filename)
                        ))
                    );

                    if (!ephemeralTab) {
                        // Even if no ephemeral tab matches, we might still want to open the file 
                        // if it's a new file or important. But for now, we only replace if found.
                        processingFilesRef.current.delete(file_path);
                        return prev;
                    }

                    console.debug('[LAYOUT] Found matching ephemeral tab, converting to file tab:', ephemeralTab.id, '→', file_path);
                    const fileTab: Tab = {
                        id: `file-${file_path}`,
                        title: filename,
                        type: 'file',
                        path: file_path,
                    };

                    // Remove the ephemeral tab and add the new file tab
                    // We try to keep the same position in the tab bar
                    const newTabs = prev.filter(t => t.id !== ephemeralTab.id);
                    return [...newTabs, fileTab];
                });

                // Keep current focus while surfacing the converted file tab in the background.
                setActiveTabId(prev => prev ?? `file-${file_path}`);

                // Clear the processing flag after a short delay to allow the open-file event to be ignored
                setTimeout(() => {
                    processingFilesRef.current.delete(file_path);
                }, 500);
            }));

        return () => {
            for (const unlistenPromise of unlistenPromises) {
                unlistenPromise
                    .then(unlisten => unlisten())
                    .catch(console.error);
            }
        };
    }, [chat.setConversation]);

    useEffect(() => {
        if (chat.loading) {
            setWasStoppedByUser(false);
        }
    }, [chat.loading]);

    // Clear active indicator when chat stops loading and finalize any lingering
    // non-terminal research activity cards so they don't keep spinning forever.
    useEffect(() => {
        if (!chat.loading) {
            setResearchProgress(prev => prev ? { ...prev, isActive: false } : null);
            const finalStage = wasStoppedByUser ? 'STOPPED' : 'COMPLETE';

            chat.setConversation(prev => {
                let changed = false;
                const next = prev.map(msg => {
                    const activities = msg.researchActivities;
                    if (!activities || activities.length === 0) return msg;

                    const updatedActivities = activities.map(activity => {
                        const stage = activity.stage.toLowerCase();
                        const isTerminalStage =
                            stage.includes('complete')
                            || stage.includes('done')
                            || stage.includes('error')
                            || stage.includes('fail')
                            || stage.includes('cancel')
                            || stage.includes('stop');

                        if (isTerminalStage) return activity;

                        changed = true;
                        return {
                            ...activity,
                            stage: finalStage,
                            percent: 100,
                        };
                    });

                    if (!changed) return msg;

                    return {
                        ...msg,
                        researchActivities: updatedActivities,
                    };
                });

                return changed ? next : prev;
            });
        }
    }, [chat.loading, chat.setConversation, wasStoppedByUser]);

    // Measure editor column width so AppBar can constrain the tab strip to it
    const editorColumnRef = useRef<HTMLDivElement>(null);
    const [editorColumnWidth, setEditorColumnWidth] = useState<number | undefined>(undefined);
    useEffect(() => {
        const el = editorColumnRef.current;
        if (!el) return;
        const ro = new ResizeObserver(entries => {
            setEditorColumnWidth(entries[0].contentRect.width);
        });
        ro.observe(el);
        return () => ro.disconnect();
    }, []);

    // Toggle sidebar function
    const toggleSidebar = (view: 'explorer' | 'git' | 'history') => {
        if (isSidebarOpen && activeSidebar === view) {
            setIsSidebarOpen(false);
        } else {
            setActiveSidebar(view);
            setIsSidebarOpen(true);
        }
    };

    const editorViewportBottomInset = terminalHeight > 0 ? terminalHeight : 0;

    return (
        <div className="h-screen w-screen bg-[var(--bg-app)] overflow-hidden flex flex-col font-sans text-[var(--fg-primary)]">
            {/* Unified App Bar: title bar + tab strip merged */}
            <AppBar
                tabs={appBarTabs}
                activeTabId={activeTabId}
                projectName={projectName}
                onTabClick={handleTabClick}
                onTabClose={handleTabClose}
                onReorder={(fromIndex, toIndex) => {
                    setTabs(prev => {
                        const newTabs = [...prev];
                        const [movedTab] = newTabs.splice(fromIndex, 1);
                        newTabs.splice(toIndex, 0, movedTab);
                        return newTabs;
                    });
                }}
                tabStripMaxWidth={editorColumnWidth}
            />

            <div
                className="flex-1 flex overflow-hidden relative"
                style={{ padding: 'var(--panel-gap)', gap: 'var(--panel-gap)' }}
            >
                {/* Activity Bar (Vertical) — floating pill */}
                <div
                    className="flex flex-col items-center py-4 gap-6 z-50 shrink-0 relative"
                    style={{
                        width: '46px',
                        backgroundColor: 'var(--bg-panel)',
                        borderRadius: '0',
                        border: '1px solid var(--border-default)',
                        boxShadow: 'var(--panel-shadow)',
                    }}
                >
                    <div
                        onClick={() => toggleSidebar('explorer')}
                        title="Explorer"
                        aria-label="Explorer"
                        className={`relative p-2 rounded-md cursor-pointer transition-all duration-[var(--transition-fast)] ${isSidebarOpen && activeSidebar === 'explorer'
                            ? 'text-[var(--fg-bright)] bg-[var(--bg-surface)]'
                            : 'text-[var(--fg-nav)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface)]'}
                        `}
                    >
                        {isSidebarOpen && activeSidebar === 'explorer' && (
                            <div className="absolute left-0 top-0 bottom-0 w-[2px] bg-[var(--accent-primary)] rounded-r" />
                        )}
                        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                        </svg>
                    </div>
                    <div
                        onClick={() => toggleSidebar('git')}
                        title="Git"
                        aria-label="Git"
                        className={`relative p-2 rounded-md cursor-pointer transition-all duration-[var(--transition-fast)] ${isSidebarOpen && activeSidebar === 'git'
                            ? 'text-[var(--fg-bright)] bg-[var(--bg-surface)]'
                            : 'text-[var(--fg-nav)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface)]'}
                        `}
                    >
                        {isSidebarOpen && activeSidebar === 'git' && (
                            <div className="absolute left-0 top-0 bottom-0 w-[2px] bg-[var(--accent-primary)] rounded-r" />
                        )}
                        <GitBranch className="w-5 h-5" />
                        {gitStatus?.isRepo && gitChangedCount > 0 && (
                            <span className="absolute -bottom-1 -right-1 min-w-[14px] h-3 px-1 rounded-full bg-[var(--accent-primary)] text-[9px] leading-3 text-white text-center shadow-sm">
                                {Math.min(gitChangedCount, 99)}
                            </span>
                        )}
                    </div>
                    <div
                        onClick={() => toggleSidebar('history')}
                        title="File History"
                        aria-label="File History"
                        className={`relative p-2 rounded-md cursor-pointer transition-all duration-[var(--transition-fast)] ${isSidebarOpen && activeSidebar === 'history'
                            ? 'text-[var(--fg-bright)] bg-[var(--bg-surface)]'
                            : 'text-[var(--fg-nav)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface)]'}
                        `}
                    >
                        {isSidebarOpen && activeSidebar === 'history' && (
                            <div className="absolute left-0 top-0 bottom-0 w-[2px] bg-[var(--accent-primary)] rounded-r" />
                        )}
                        <Clock className="w-5 h-5" />
                    </div>
                    <div
                        title="Search (coming soon)"
                        aria-label="Search"
                        className="hidden relative p-2 rounded-md text-[var(--fg-nav)] opacity-40 cursor-not-allowed transition-all duration-[var(--transition-fast)]"
                    >
                        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                        </svg>
                    </div>
                    <div
                        onClick={() => setIsSettingsOpen(true)}
                        title="Settings"
                        aria-label="Settings"
                        className="relative mt-auto p-2 rounded-md text-[var(--fg-nav)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface)] transition-all duration-[var(--transition-fast)] cursor-pointer"
                    >
                        <Settings className="w-5 h-5" />
                    </div>
                </div>

                {/* Explorer / Sidebar (Floating overlay — anchored to outer row, above all editor content) */}
                <div
                    className={`
                        absolute top-0 bottom-0 w-80 bg-[var(--bg-panel)] flex flex-col overflow-hidden
                        transition-all duration-[var(--transition-fast)] ease-[cubic-bezier(0.22,1,0.36,1)]
                        ${isSidebarOpen
                            ? 'opacity-100 visible'
                            : 'opacity-0 invisible pointer-events-none'}
                    `}
                    style={{
                        left: 'calc(46px + 2 * var(--panel-gap))',
                        borderRadius: '0',
                        border: '1px solid var(--border-default)',
                        boxShadow: isSidebarOpen ? 'var(--panel-shadow)' : 'none',
                        zIndex: 200,
                        transform: isSidebarOpen ? 'translateX(0)' : 'translateX(-100%)',
                    }}
                >
                    {activeSidebar === 'explorer' && (
                        <ExplorerPanel onFileSelect={handleFileSelect} activeFile={tabs.find(t => t.id === activeTabId)?.path || null} />
                    )}
                    {activeSidebar === 'git' && (
                        <Suspense fallback={<div className="h-full flex items-center justify-center text-[var(--fg-subtle)]">Loading Git...</div>}>
                            <GitPanel
                                status={gitStatus}
                                files={gitFiles}
                                error={gitError}
                                filesError={gitFilesError}
                                lastRefreshedAt={gitLastRefreshedAt}
                                onRefresh={refreshGitStatus}
                                onStageFile={stageGitFile}
                                onUnstageFile={unstageGitFile}
                                onStageAll={stageAllGit}
                                onUnstageAll={unstageAllGit}
                                onCommit={commitGit}
                                onPush={pushGit}
                                onDiff={diffGit}
                                onGenerateCommitMessage={() => generateGitCommitMessage(selectedModelId)}
                                onCommitPreflight={commitPreflightGit}
                            />
                        </Suspense>
                    )}
                    {activeSidebar === 'history' && (
                        <Suspense fallback={<div className="h-full flex items-center justify-center text-[var(--fg-subtle)]">Loading History...</div>}>
                            <FileHistoryPanel activeFile={tabs.find(t => t.id === activeTabId)?.path || null} />
                        </Suspense>
                    )}
                </div>

                {/* Content Area */}
                <div className="flex-1 flex min-w-0 overflow-hidden" style={{ gap: 'var(--panel-gap)' }}>

                    {/* Editor & Terminal — floating card */}
                    <div
                        ref={editorColumnRef}
                        className="flex-1 flex flex-col min-w-0 relative overflow-hidden"
                        style={{
                            backgroundColor: 'var(--bg-editor)',
                            borderRadius: 'var(--panel-radius)',
                            border: '1px solid var(--border-default)',
                            boxShadow: 'var(--panel-shadow)',
                        }}
                    >

                        <div
                            className="flex-1 overflow-hidden relative"
                        >
                            {(() => {
                                const activeTab = tabs.find(t => t.id === activeTabId);

                                return (
                                    <>
                                        {/* Render all file tabs (hidden when not active) */}
                                        {tabs.filter(t => t.type === 'file').map(tab => {
                                            const isActive = tab.id === activeTabId;

                                            return (
                                                <div
                                                    key={tab.id}
                                                    className={`absolute inset-x-0 top-0 ${isActive ? 'z-10' : 'z-0 pointer-events-none opacity-0'}`}
                                                    style={{ bottom: editorViewportBottomInset }}
                                                >
                                                    <EditorPanel
                                                        activeFile={tab.path || null}
                                                        highlightLines={tab.highlightLines || null}
                                                        onOpenSettings={() => setIsSettingsOpen(true)}
                                                    />
                                                </div>
                                            );
                                        })}

                                        {/* Render Welcome Page if no tabs */}
                                        {tabs.length === 0 && (
                                            <div className="absolute inset-x-0 top-0 z-10" style={{ bottom: editorViewportBottomInset }}>
                                                <EditorPanel
                                                    activeFile={null}
                                                    onOpenSettings={() => setIsSettingsOpen(true)}
                                                />
                                            </div>
                                        )}

                                        {/* Render ephemeral tabs */}
                                        {tabs.filter(t => t.type === 'ephemeral').map(tab => {
                                            const isActive = tab.id === activeTabId;
                                            return (
                                                <div
                                                    key={tab.id}
                                                    className={`absolute inset-x-0 top-0 ${isActive ? 'z-10' : 'z-0 pointer-events-none opacity-0'}`}
                                                    style={{ bottom: editorViewportBottomInset }}
                                                >
                                                    <DocumentViewer
                                                        documentId={tab.id}
                                                        title={tab.title}
                                                        content={tab.content || ''}
                                                        isEphemeral={true}
                                                        suggestedName={tab.suggestedName}
                                                        onClose={() => handleTabClose(tab.id)}
                                                        onSave={(savedPath) => handleEphemeralSave(tab.id, savedPath)}
                                                    />
                                                </div>
                                            );
                                        })}

                                    </>
                                );
                            })()}
                        </div>

                        {/* Terminal Drawer — floats over the editor from the bottom */}
                        <div
                            className="absolute bottom-0 left-0 right-0 z-20 flex flex-col overflow-hidden"
                            style={{
                                height: terminalHeight,
                                backgroundColor: 'var(--term-bg)',
                                borderRadius: `0 0 var(--panel-radius) var(--panel-radius)`,
                                boxShadow: '0 -10px 28px rgba(0,0,0,0.45)',
                                transform: terminalHeight > 0 ? 'translateY(0)' : 'translateY(100%)',
                                transition: isTerminalDragging ? 'none' : 'transform var(--transition-base)',
                            }}
                        >
                            {/* Drag handle strip */}
                            <div
                                className="relative h-3 shrink-0 group flex items-center resize-y-cursor select-none"
                                onMouseDown={handleTerminalMouseDown}
                            >
                                <div
                                    className={`w-full transition-all duration-[var(--transition-fast)] ${isTerminalDragging
                                        ? 'h-[2px] bg-[var(--accent-primary)]'
                                        : 'h-px bg-[var(--border-subtle)] group-hover:h-[2px] group-hover:bg-[var(--accent-primary)]'
                                    }`}
                                />
                            </div>
                            <div className="flex-1 min-h-0 overflow-hidden">
                                <TerminalPane ref={terminalPaneRef} />
                            </div>
                        </div>
                    </div>

                    {/* Chat Panel Resizer */}
                    <div
                        className="relative w-0 z-40 shrink-0 select-none pointer-events-none"
                        style={{
                            marginLeft: 'calc(var(--panel-gap) / -2)',
                            marginRight: 'calc(var(--panel-gap) / -2)',
                        }}
                    >
                        <div
                            className={`absolute inset-y-0 left-1/2 -translate-x-1/2 flex items-center justify-center pointer-events-auto resize-x-cursor transition-colors duration-[var(--transition-fast)] ${isChatDragging
                                ? 'bg-[var(--accent-primary)]/10'
                                : 'bg-transparent hover:bg-[var(--accent-primary)]/10'
                                }`}
                            style={{ width: 'var(--panel-gap)' }}
                            onMouseDown={handleChatMouseDown}
                        >
                            <div className={`h-full w-px ${isChatDragging ? 'bg-[var(--accent-primary)]' : 'bg-transparent hover:bg-[var(--accent-primary)]'}`} />
                        </div>
                    </div>

                    {/* AI Chat — floating card */}
                    <div
                        style={{
                            width: chatPanelWidth,
                            backgroundColor: 'var(--bg-panel)',
                            borderRadius: '0',
                            border: '1px solid var(--border-default)',
                            boxShadow: 'var(--panel-shadow)',
                        }}
                        className="min-w-[280px] max-w-[800px] flex flex-col z-30 overflow-hidden"
                    >
                        <Suspense fallback={<div className="flex-1 bg-[var(--bg-panel)] h-full w-full" />}>
                            <ChatPanel
                                messages={chat.messages}
                                loading={chat.loading}
                                error={chat.error}
                                sendMessage={chat.sendMessage}
                                stopGeneration={handleStopGeneration}
                                models={chat.models}
                                selectedModelId={chat.selectedModelId}
                                setSelectedModelId={chat.setSelectedModelId}
                                pendingActions={chat.pendingActions}
                                approveToolDecision={chat.approveToolDecision}
                                skipSingleCommand={chat.skipSingleCommand}

                                projectId={projectId || "default-project"}
                                onLoadConversation={chat.loadConversation}
                                researchProgress={researchProgress}
                                onNewConversation={chat.newConversation}
                                onUndoTool={chat.undoTool}
                                uncommittedChanges={uncommittedChanges}
                                onAcceptAllChanges={acceptAllChanges}
                                onRejectAllChanges={rejectAllChanges}
                                toolActivity={chat.toolActivity}
                                activeTodos={chat.activeTodos}
                                setActiveTodos={chat.setActiveTodos}
                                queuedRequests={chat.messageQueue}
                                deleteQueuedRequest={chat.deleteQueuedRequest}
                            />
                        </Suspense>
                    </div>

                </div>
            </div>

            {/* Status Bar */}
            <div
                className="text-[var(--fg-tertiary)] flex items-center px-3 text-[10px] font-mono justify-between select-none z-40"
                style={{
                    height: '24px',
                    backgroundColor: 'var(--bg-panel)',
                    margin: '0 var(--panel-gap) var(--panel-gap)',
                    borderRadius: '0',
                    border: '1px solid var(--border-default)',
                    boxShadow: 'var(--panel-shadow)',
                }}
            >
                <div className="flex items-center gap-1.5">
                    <span className="flex items-center gap-1.5 hover:text-[var(--fg-secondary)] cursor-pointer transition-colors duration-[var(--transition-fast)]">
                        <GitBranch className="w-3 h-3" />
                        {gitStatus?.branch ?? 'no branch'}{gitStatus?.dirty ? '*' : ''}
                    </span>
                </div>
                <div className="flex items-center gap-4 opacity-70">
                    {/* Saving Indicator */}
                    {isClosing && (
                        <span className="text-emerald-500 animate-pulse font-semibold">Saving...</span>
                    )}
                    <span>{t('editor.encoding')}</span>
                    <span>{(() => {
                        const activeTab = tabs.find(tab => tab.id === activeTabId);
                        if (!activeTab?.path) return null;
                        const ext = activeTab.path.split('.').pop()?.toLowerCase();
                        const langMap: Record<string, string> = {
                            rs: 'Rust', ts: 'TypeScript', tsx: 'TypeScript React', js: 'JavaScript', jsx: 'JavaScript React',
                            py: 'Python', rb: 'Ruby', go: 'Go', java: 'Java', kt: 'Kotlin', swift: 'Swift',
                            c: 'C', cpp: 'C++', h: 'C Header', hpp: 'C++ Header', cs: 'C#',
                            html: 'HTML', css: 'CSS', scss: 'SCSS', less: 'Less', json: 'JSON', yaml: 'YAML', yml: 'YAML',
                            xml: 'XML', md: 'Markdown', toml: 'TOML', sql: 'SQL', sh: 'Shell', bash: 'Bash',
                            lua: 'Lua', zig: 'Zig', dart: 'Dart', php: 'PHP', r: 'R',
                            svelte: 'Svelte', vue: 'Vue', astro: 'Astro',
                            txt: 'Plain Text', csv: 'CSV', svg: 'SVG',
                        };
                        return langMap[ext ?? ''] ?? ext?.toUpperCase() ?? null;
                    })()}</span>
                    <span>{t('app.name')}</span>
                </div>
            </div>

            {/* Dev Tools */}
            <Suspense fallback={null}>
                <ProtocolExplorer />
            </Suspense>

            {/* Settings Modal */}
            <Suspense fallback={null}>
                {isSettingsOpen && (
                    <SettingsModal
                        isOpen={isSettingsOpen}
                        onClose={() => setIsSettingsOpen(false)}
                        workspacePath={workspacePath}
                        onRefreshModels={refreshModels}
                    />
                )}
            </Suspense>

            {/* First-time Storage Setup Modal (RFC-002) */}
            {workspacePath && (
                <StorageSetupModal
                    isOpen={showStorageSetup}
                    workspacePath={workspacePath}
                    onComplete={() => setShowStorageSetup(false)}
                />
            )}
        </div>
    );
};

export const AppLayout: React.FC = () => {
    return (
        <EditorProvider>
            <AppLayoutInner />
        </EditorProvider>
    );
};
