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
import { useTabManager, type Tab } from '../hooks/useTabManager';
import { useResizeHandlers } from '../hooks/useResizeHandlers';
import { useLayoutEvents } from '../hooks/useLayoutEvents';
import type { BackendSettings } from '../types/settings';
const ChatPanel = React.lazy(() => import('./ChatPanel').then(module => ({ default: module.ChatPanel })));
const GitPanel = React.lazy(() => import('./GitPanel').then(module => ({ default: module.GitPanel })));
const FileHistoryPanel = React.lazy(() => import('./FileHistoryPanel').then(module => ({ default: module.FileHistoryPanel })));
const SettingsModal = React.lazy(() => import('./SettingsModal').then(module => ({ default: module.SettingsModal })));
const ProtocolExplorer = React.lazy(() => import('./dev/ProtocolExplorer').then(module => ({ default: module.ProtocolExplorer })));

function normalizePath(value: string): string {
    return value.replace(/\\/g, '/').replace(/\/+/g, '/').replace(/\/$/, '');
}

function isBoundarySuffixMatch(full: string, suffix: string): boolean {
    if (!full.endsWith(suffix)) return false;
    if (full.length === suffix.length) return true;
    return full[full.length - suffix.length - 1] === '/';
}

function findMatchingChangeRange(
    filePath: string,
    changes: { file_path: string; unified_diff: string; timestamp: number }[]
): { startLine: number; endLine: number } | null {
    const target = normalizePath(filePath);
    const matches = changes
        .filter((change) => {
            const candidate = normalizePath(change.file_path);
            return candidate === target
                || isBoundarySuffixMatch(candidate, target)
                || isBoundarySuffixMatch(target, candidate);
        })
        .sort((a, b) => b.timestamp - a.timestamp);

    const change = matches.find((item) => item.unified_diff.trim().length > 0) ?? matches[0];
    if (!change) return null;

    const hunkMatch = change.unified_diff.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@/m);
    if (!hunkMatch) return null;

    const startLine = Number.parseInt(hunkMatch[1], 10);
    const lineCount = Number.parseInt(hunkMatch[2] ?? '1', 10);
    if (Number.isNaN(startLine) || Number.isNaN(lineCount)) return null;

    const safeLineCount = Math.max(1, lineCount);
    return {
        startLine,
        endLine: startLine + safeLineCount - 1,
    };
}

const AppLayoutInner: React.FC = () => {
    const { t } = useTranslation();
    const appWindow = getCurrentWindow();

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
    const { selectedModelId, setSelectedModelId, refreshModels } = chat;
    const terminalPaneRef = useRef<TerminalPaneHandle>(null);

    const handleStopGeneration = useCallback(async () => {
        setWasStoppedByUser(true);
        await chat.stopGeneration();
    }, [chat.stopGeneration]);

    // Tab management (CRUD, history, keyboard shortcuts, backend sync)
    const tabManager = useTabManager(uncommittedChanges);
    const {
        tabs, setTabs, activeTabId, setActiveTabId, activeTab, activeFilename,
        appBarTabs, setAiEditedFilePaths, setUnseenAiEditedFilePaths,
        processingFilesRef, handleTabClick, handleFileSelect, handleTabClose,
        handleTabReorder, handleEphemeralSave,
    } = tabManager;

    // Sync active tab and open file paths to EditorContext
    const { setActiveFile, setOpenFiles } = useEditorActions();
    useEffect(() => {
        const tab = tabs.find(t => t.id === activeTabId);
        setActiveFile(tab?.path || null);
    }, [activeTabId, tabs, setActiveFile]);

    useEffect(() => {
        const filePaths = tabs
            .filter(t => t.type === 'file' && t.path)
            .map(t => t.path!);
        setOpenFiles(filePaths);
    }, [tabs, setOpenFiles]);

    const handleOpenChatFile = useCallback((path: string) => {
        const highlightLines = findMatchingChangeRange(path, uncommittedChanges);
        handleFileSelect(path, highlightLines);
    }, [handleFileSelect, uncommittedChanges]);

    const handleExplorerFileSelect = useCallback((path: string, _line?: number, _character?: number) => {
        handleFileSelect(path);
    }, [handleFileSelect]);

    // Tauri event listeners (file open, research progress, change-applied, etc.)
    const { researchProgress, finalizeResearchActivities } = useLayoutEvents({
        setTabs,
        setActiveTabId,
        setAiEditedFilePaths,
        setUnseenAiEditedFilePaths,
        processingFilesRef,
        setConversation: chat.setConversation,
    });

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

    // Resize handlers (terminal + chat panel drag) — must be before handleStateLoaded
    const editorColumnRef = useRef<HTMLDivElement>(null);
    const {
        terminalHeight, setTerminalHeight,
        chatPanelWidth, setChatPanelWidth,
        isTerminalDragging, isChatDragging,
        handleTerminalMouseDown, handleChatMouseDown,
    } = useResizeHandlers({ editorColumnRef });

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
                const restoredActive = restoredTabs.find(t => t.path === state.active_file);
                if (restoredActive) {
                    setActiveTabId(restoredActive.id);
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
    }, [setSelectedModelId, setTerminalHeight, setChatPanelWidth, setTabs, setActiveTabId]);

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

    useEffect(() => {
        if (chat.loading) {
            setWasStoppedByUser(false);
        }
    }, [chat.loading]);

    // Finalize research activities when chat stops loading
    useEffect(() => {
        finalizeResearchActivities(chat.loading, wasStoppedByUser);
    }, [chat.loading, wasStoppedByUser, finalizeResearchActivities]);

    // Measure editor column width so AppBar can constrain the tab strip to it
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
                onReorder={handleTabReorder}
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
                        title={t('activityBar.explorer')}
                        aria-label={t('activityBar.explorer')}
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
                        title={t('activityBar.git')}
                        aria-label={t('activityBar.git')}
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
                        title={t('activityBar.fileHistory')}
                        aria-label={t('activityBar.fileHistory')}
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
                        title={t('activityBar.searchComingSoon')}
                        aria-label={t('activityBar.search')}
                        className="hidden relative p-2 rounded-md text-[var(--fg-nav)] opacity-40 cursor-not-allowed transition-all duration-[var(--transition-fast)]"
                    >
                        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                        </svg>
                    </div>
                    <div
                        onClick={() => setIsSettingsOpen(true)}
                        title={t('activityBar.settings')}
                        aria-label={t('activityBar.settings')}
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
                        <ExplorerPanel onFileSelect={handleExplorerFileSelect} activeFile={tabs.find(t => t.id === activeTabId)?.path || null} />
                    )}
                    {activeSidebar === 'git' && (
                        <Suspense fallback={<div className="h-full flex items-center justify-center text-[var(--fg-subtle)]">{t('sidebar.loadingGit')}</div>}>
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
                        <Suspense fallback={<div className="h-full flex items-center justify-center text-[var(--fg-subtle)]">{t('sidebar.loadingHistory')}</div>}>
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
                                onOpenFile={handleOpenChatFile}
                                uncommittedChanges={uncommittedChanges}
                                onAcceptAllChanges={acceptAllChanges}
                                onRejectAllChanges={rejectAllChanges}
                                toolActivity={chat.toolActivity}
                                activeTodos={chat.activeTodos}
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
                        {gitStatus?.branch ?? t('statusBar.noBranch')}{gitStatus?.dirty ? '*' : ''}
                    </span>
                </div>
                <div className="flex items-center gap-4 opacity-70">
                    {/* Saving Indicator */}
                    {isClosing && (
                        <span className="text-emerald-500 animate-pulse font-semibold">{t('statusBar.saving')}</span>
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
