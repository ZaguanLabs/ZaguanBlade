import React, { useState, useEffect, useRef, useCallback, Suspense } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { ExplorerPanel } from './ExplorerPanel';
import { EditorPanel } from './EditorPanel';
import { TerminalPane, TerminalPaneHandle } from './TerminalPane';
import { DocumentTabs } from './DocumentTabs';
import { DocumentViewer } from './DocumentViewer';
import { TitleBar } from './TitleBar';
import { GitBranch, Settings, Clock } from 'lucide-react';
import { EditorProvider, useEditor } from '../contexts/EditorContext';
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
    const [activeTabId, setActiveTabId] = useState<string | null>(null);
    const [tabs, setTabs] = useState<Tab[]>([]);
    const [terminalHeight, setTerminalHeight] = useState(300);
    const [chatPanelWidth, setChatPanelWidth] = useState(400);
    const [isDragging, setIsDragging] = useState(false);
    const [isChatDragging, setIsChatDragging] = useState(false);


    // Sidebar State
    const [isSidebarOpen, setIsSidebarOpen] = useState(false);
    const [activeSidebar, setActiveSidebar] = useState<'explorer' | 'git' | 'history'>('explorer');

    const chat = useChat();
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

    // Sync active tab to EditorContext
    const { setActiveFile } = useEditor();
    useEffect(() => {
        const activeTab = tabs.find(t => t.id === activeTabId);
        setActiveFile(activeTab?.path || null);
    }, [activeTabId, tabs, setActiveFile]);

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
                    setTabs(prev => prev.filter(t => t.id !== closedId));
                    setActiveTabId(prev => prev === closedId ? null : prev);
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

    // Fetch project settings and sync to EditorContext
    useEffect(() => {
        if (!workspacePath) return;

        const loadSettings = async () => {
            try {
                const settings = await invoke<BackendSettings>('load_project_settings', {
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
        console.log('[Layout] Restoring project state:', state);

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



    // Auto-open files when edit proposals arrive
    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;

        let unlisten: (() => void) | undefined;

        const setupListener = async () => {
            unlisten = await listen<{ id: string; path: string; old_content: string; new_content: string }[]>('propose-edit', (event) => {
                if (event.payload.length > 0) {
                    const firstEdit = event.payload[0];
                    const existingTab = tabs.find(t => t.type === 'file' && t.path === firstEdit.path);

                    if (existingTab) {
                        setActiveTabId(existingTab.id);
                    } else {
                        const filename = firstEdit.path.split('/').pop() || firstEdit.path;
                        const newTab: Tab = {
                            id: `file-${firstEdit.path}`,
                            title: filename,
                            type: 'file',
                            path: firstEdit.path,
                        };
                        setTabs(prev => [...prev, newTab]);
                        setActiveTabId(newTab.id);
                    }
                }
            });
        };

        setupListener();

        return () => {
            if (unlisten) unlisten();
        };
    }, [tabs]);

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
            setTabs(prev => prev.filter(t => t.id !== tabId));
            if (activeTabId === tabId) {
                setActiveTabId(tabs.length > 1 ? tabs[0].id : null);
            }
        }
    };

    const handleEphemeralSave = async (ephemeralTabId: string, savedPath: string) => {
        console.log('[Layout] handleEphemeralSave called:', { ephemeralTabId, savedPath });
        
        // Convert ephemeral tab to regular file tab
        setTabs(prev => {
            const ephemeralTab = prev.find(t => t.id === ephemeralTabId);
            if (!ephemeralTab) {
                console.log('[Layout] Ephemeral tab not found:', ephemeralTabId);
                return prev;
            }

            console.log('[Layout] Found ephemeral tab:', ephemeralTab);
            const filename = savedPath.split('/').pop() || savedPath;
            const newTab: Tab = {
                id: `file-${savedPath}`,
                title: filename,
                type: 'file',
                path: savedPath,
            };

            console.log('[Layout] Creating new file tab:', newTab);
            // Remove ephemeral tab and add file tab
            return [...prev.filter(t => t.id !== ephemeralTabId), newTab];
        });

        // Switch to the new file tab
        const newTabId = `file-${savedPath}`;
        console.log('[Layout] Switching to new tab:', newTabId);
        setActiveTabId(newTabId);

        // Trigger backend to open the file so it loads in the editor
        try {
            console.log('[Layout] Calling open_file_in_editor:', savedPath);
            await invoke('open_file_in_editor', { path: savedPath });
            console.log('[Layout] open_file_in_editor completed successfully');
        } catch (error) {
            console.error('[Layout] Failed to open saved file:', error);
        }
    };

    const handleMouseDown = (e: React.MouseEvent) => {
        setIsDragging(true);
        e.preventDefault();
    };

    useEffect(() => {
        const handleMouseMove = (e: MouseEvent) => {
            if (!isDragging) return;
            // Calculate new height from bottom
            // Height = TotalWindowHeight - MouseY - StatusBarHeight(24px)
            const newHeight = window.innerHeight - e.clientY - 24;
            // Clamp height
            if (newHeight > 100 && newHeight < window.innerHeight - 100) {
                setTerminalHeight(newHeight);
            }
        };

        const handleMouseUp = () => {
            setIsDragging(false);
        };

        if (isDragging) {
            document.addEventListener('mousemove', handleMouseMove);
            document.addEventListener('mouseup', handleMouseUp);
            document.body.style.cursor = 'row-resize';
        }

        return () => {
            document.removeEventListener('mousemove', handleMouseMove);
            document.removeEventListener('mouseup', handleMouseUp);
            document.body.style.removeProperty('cursor');
        };
    }, [isDragging]);

    // Chat panel resize handler
    const handleChatMouseDown = (e: React.MouseEvent) => {
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
            document.body.style.cursor = 'col-resize';
        }

        return () => {
            document.removeEventListener('mousemove', handleChatMouseMove);
            document.removeEventListener('mouseup', handleChatMouseUp);
            document.body.style.removeProperty('cursor');
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

        let unlistenFile: (() => void) | undefined;
        let unlistenFileOpened: (() => void) | undefined;
        let unlistenFileWithHighlight: (() => void) | undefined;
        let unlistenEphemeral: (() => void) | undefined;
        let unlistenResearchProgress: (() => void) | undefined;
        let unlistenChangeApplied: (() => void) | undefined;
        let unlistenChatError: (() => void) | undefined;

        const setupListeners = async () => {
            const handleOpenFile = (path: string, sourceEvent: string) => {
                console.log(`Opening file from backend (${sourceEvent}):`, path);
                const tabId = `file-${path}`;

                // Prevent duplicate processing
                if (processingFilesRef.current.has(path)) {
                    console.log('[LAYOUT] Ignoring duplicate file open event for:', path);
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
                setActiveTabId(tabId);
            };

            // Current backend event name (Rust emits this)
            unlistenFile = await listen<string>('open-file', (event) => {
                handleOpenFile(event.payload, 'open-file');
            });

            // Backwards-compatible alias (kept for older emitters)
            unlistenFileOpened = await listen<string>('file-opened', (event) => {
                handleOpenFile(event.payload, 'file-opened');
            });

            unlistenFileWithHighlight = await listen<{ path: string; start_line: number; end_line: number }>('open-file-with-highlight', (event) => {
                console.log('Opening file with highlight from backend:', event.payload);
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
                setActiveTabId(tabId);
            });

            unlistenEphemeral = await listen<{ id: string; title: string; content: string; suggestedName: string }>('open-ephemeral-document', (event) => {
                console.log('[LAYOUT] 📥 Received open-ephemeral-document event:', {
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
                        console.log('[LAYOUT] ⚠️ Tab already exists, just activating:', id);
                        return prev;
                    }

                    console.log('[LAYOUT] ✅ Creating new tab with ID:', id);
                    const newTab: Tab = {
                        id,
                        title,
                        type: 'ephemeral',
                        content,
                        suggestedName,
                    };
                    console.log('[LAYOUT] Adding tab to existing tabs:', prev.length, '→', prev.length + 1);
                    return [...prev, newTab];
                });
                setActiveTabId(id);
            });

            // Listen for research progress events
            unlistenResearchProgress = await listen<{ message: string; stage: string; percent: number }>('research-progress', (event) => {
                console.log('[LAYOUT] Research progress:', event.payload);
                
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
            });

            // Listen for chat errors to clear progress
            unlistenChatError = await listen('chat-error', () => {
                setResearchProgress(null);
            });

            // NOTE: context-length-exceeded is now handled in useChat.ts where it belongs

            // Listen for change-applied events to convert ephemeral tabs to file tabs
            unlistenChangeApplied = await listen<{ change_id: string; file_path: string }>('change-applied', (event) => {
                console.log('[LAYOUT] Change applied:', event.payload);
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

                    console.log('[LAYOUT] Found matching ephemeral tab, converting to file tab:', ephemeralTab.id, '→', file_path);
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

                // Switch to the new file tab
                setActiveTabId(`file-${file_path}`);

                // Clear the processing flag after a short delay to allow the open-file event to be ignored
                setTimeout(() => {
                    processingFilesRef.current.delete(file_path);
                }, 500);
            });
        };

        setupListeners();

        return () => {
            if (unlistenFile) unlistenFile();
            if (unlistenFileOpened) unlistenFileOpened();
            if (unlistenFileWithHighlight) unlistenFileWithHighlight();
            if (unlistenEphemeral) unlistenEphemeral();
            if (unlistenResearchProgress) unlistenResearchProgress();
            if (unlistenChangeApplied) unlistenChangeApplied();
            if (unlistenChatError) unlistenChatError();
        };
    }, [tabs]);

    // Clear active indicator when chat stops loading (but keep in message history)
    useEffect(() => {
        if (!chat.loading) {
            setResearchProgress(prev => prev ? { ...prev, isActive: false } : null);
        }
    }, [chat.loading]);

    // Toggle sidebar function
    const toggleSidebar = (view: 'explorer' | 'git' | 'history') => {
        if (isSidebarOpen && activeSidebar === view) {
            setIsSidebarOpen(false);
        } else {
            setActiveSidebar(view);
            setIsSidebarOpen(true);
        }
    };

    return (
        <div className="h-screen w-screen bg-[var(--bg-app)] overflow-hidden flex flex-col font-sans text-[var(--fg-primary)]">
            {/* Custom Title Bar with Window Controls */}
            <TitleBar />

            <div className="flex-1 flex overflow-hidden">
                {/* Activity Bar (Vertical) */}
                <div className="w-[50px] bg-[var(--bg-panel)] border-r border-[var(--border-subtle)] flex flex-col items-center py-4 gap-6 z-50 shrink-0 relative">
                    <div
                        onClick={() => toggleSidebar('explorer')}
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
                    <div className="relative p-2 rounded-md text-[var(--fg-nav)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface)] transition-all duration-[var(--transition-fast)] cursor-pointer">
                        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                        </svg>
                    </div>
                    <div
                        onClick={() => setIsSettingsOpen(true)}
                        className="relative mt-auto p-2 rounded-md text-[var(--fg-nav)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface)] transition-all duration-[var(--transition-fast)] cursor-pointer"
                    >
                        <Settings className="w-5 h-5" />
                    </div>
                </div>

                {/* Content Area */}
                <div className="flex-1 flex min-w-0 relative">

                    {/* Explorer / Sidebar (Floating) */}
                    <div className={`
                        absolute top-0 bottom-0 left-0 w-80 bg-[var(--bg-panel)] border-r border-[var(--border-subtle)] 
                        shadow-[var(--shadow-lg)] z-30 flex flex-col
                        transition-transform duration-[var(--transition-base)] ease-out
                        ${isSidebarOpen ? 'translate-x-0' : '-translate-x-full'}
                    `}>
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

                    {/* Editor & Terminal */}
                    <div className="flex-1 flex flex-col min-w-0 bg-[var(--bg-app)] relative">
                        {/* Document Tabs */}
                        {tabs.length > 0 && (
                            <DocumentTabs
                                tabs={tabs.map(t => ({
                                    id: t.id,
                                    title: t.title,
                                    isEphemeral: t.type === 'ephemeral',
                                    isDirty: false,
                                    hasVirtualChanges: false,
                                }))}
                                activeTabId={activeTabId}
                                onTabClick={setActiveTabId}
                                onTabClose={handleTabClose}
                                onReorder={(fromIndex, toIndex) => {
                                    setTabs(prev => {
                                        const newTabs = [...prev];
                                        const [movedTab] = newTabs.splice(fromIndex, 1);
                                        newTabs.splice(toIndex, 0, movedTab);
                                        return newTabs;
                                    });
                                }}
                            />
                        )}

                        <div className="flex-1 overflow-hidden relative">
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
                                                    className={`absolute inset-0 ${isActive ? 'z-10' : 'z-0 pointer-events-none opacity-0'}`}
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
                                            <div className="absolute inset-0 z-10">
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
                                                    className={`absolute inset-0 ${isActive ? 'z-10' : 'z-0 pointer-events-none opacity-0'}`}
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

                                        {/* Show placeholder if no tabs */}
                                        {tabs.length === 0 && <EditorPanel activeFile={null} highlightLines={null} />}
                                    </>
                                );
                            })()}
                        </div>

                        {/* Terminal Resizer */}
                        <div
                            className={`h-[1px] bg-[var(--border-subtle)] hover:bg-[var(--accent-primary)] hover:h-[2px] transition-all duration-[var(--transition-fast)] z-20 ${isDragging ? 'bg-[var(--accent-primary)] h-[2px]' : ''}`}
                            style={{ cursor: 'row-resize' }}
                            onMouseDown={handleMouseDown}
                        />

                        {/* Terminal Pane */}
                        <div style={{ height: terminalHeight }} className="bg-[var(--term-bg)]">
                            <TerminalPane ref={terminalPaneRef} />
                        </div>
                    </div>

                    {/* Chat Panel Resizer */}
                    <div
                        className={`w-[3px] bg-transparent hover:bg-[var(--accent-primary)] transition-colors duration-[var(--transition-fast)] z-40 ${isChatDragging ? 'bg-[var(--accent-primary)]' : ''}`}
                        style={{ cursor: 'col-resize' }}
                        onMouseDown={handleChatMouseDown}
                    />

                    {/* AI Chat */}
                    <div
                        style={{ width: chatPanelWidth }}
                        className="min-w-[280px] max-w-[800px] border-l border-[var(--border-subtle)] bg-[var(--bg-panel)] flex flex-col shadow-xl z-30"
                    >
                        <Suspense fallback={<div className="flex-1 bg-[var(--bg-panel)] h-full w-full" />}>
                            <ChatPanel
                                messages={chat.messages}
                                loading={chat.loading}
                                error={chat.error}
                                sendMessage={chat.sendMessage}
                                stopGeneration={chat.stopGeneration}
                                models={chat.models}
                                selectedModelId={chat.selectedModelId}
                                setSelectedModelId={chat.setSelectedModelId}
                                pendingActions={chat.pendingActions}
                                approveToolDecision={chat.approveToolDecision}

                                projectId={projectId || "default-project"}
                                onLoadConversation={chat.setConversation}
                                researchProgress={researchProgress}
                                onNewConversation={chat.newConversation}
                                onUndoTool={chat.undoTool}
                                uncommittedChanges={uncommittedChanges}
                                onAcceptAllChanges={acceptAllChanges}
                                onRejectAllChanges={rejectAllChanges}
                            />
                        </Suspense>
                    </div>

                </div>
            </div>

            {/* Status Bar */}
            <div className="h-6 bg-[var(--bg-panel)] border-t border-[var(--border-subtle)] text-[var(--fg-tertiary)] flex items-center px-3 text-[10px] font-mono justify-between select-none z-40">
                <div className="relative flex-1 flex flex-col">
                    <span className="flex items-center gap-1.5 hover:text-[var(--fg-secondary)] cursor-pointer transition-colors duration-[var(--transition-fast)]">
                        <svg className="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="10" /><path d="M12 6v6l4 2" /></svg>
                        main*
                    </span>
                    <span className="text-[var(--fg-secondary)]">{(() => {
                        const activeTab = tabs.find(tab => tab.id === activeTabId);
                        return activeTab ? activeTab.title : t('editor.noFileOpen');
                    })()}</span>
                </div>
                <div className="flex items-center gap-4 opacity-70">
                    {/* Saving Indicator */}
                    {isClosing && (
                        <span className="text-emerald-500 animate-pulse font-semibold">Saving...</span>
                    )}
                    <span>{t('editor.encoding')}</span>
                    <span>Rust</span>
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
