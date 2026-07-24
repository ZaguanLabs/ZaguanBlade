import React, { useState, useEffect, useRef, useCallback, useMemo, Suspense } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open } from '@tauri-apps/plugin-dialog';
import { exists } from '@tauri-apps/plugin-fs';
import { EditorPanel, type EditorContentState } from './EditorPanel';
import { TerminalPane, TerminalPaneHandle } from './TerminalPane';
import { AppBar } from './AppBar';
import { AlertTriangle, GitBranch, Settings, Clock, Loader2 } from 'lucide-react';
import { SettingsModalLazy } from './SettingsModalLazy';
import { useStartupBootstrap } from '../contexts/StartupBootstrapContext';
import { EditorProvider, useEditorActions } from '../contexts/EditorContext';
import { useUncommittedChanges } from '../hooks/useUncommittedChanges';
import { useChatV2 } from '../hooks/useChatV2';
import { useProjectState, type ProjectState } from '../hooks/useProjectState';
import { useWarmup, type EditorWarmupSnapshot } from '../hooks/useWarmup';
import { useGitStatus } from '../hooks/useGitStatus';
import { useTabManager, type Tab } from '../hooks/useTabManager';
import { useResizeHandlers } from '../hooks/useResizeHandlers';
import { useLayoutEvents } from '../hooks/useLayoutEvents';
import { subscribeBladeNestedEventType } from '../services/bladeEvents';
import { readDebugFlag, readDebugSurfaceFlag } from '../utils/debugFlags';
import { formatIndexStatusLabel, formatIndexStatusTitle, shouldShowIndexStatusCue } from '../utils/indexHealthStatus';
import type { ChatMessage, ChatMode, ModelInfo } from '../types/chat';
import type { BackendSettings } from '../types/settings';
import type { IndexHealthSnapshot } from '../types/blade';
import type { UncommittedChange } from '../types/uncommitted';
import { recordDebugPerf } from '../utils/debugPerf';
import {
    applyEditorContentSnapshot,
    applyEditorContentSnapshotToMirror,
    applyEditorContentSnapshots,
    getDirtyEditorSaveCandidateDisplayNames,
    getEditorContentSnapshotForMirror,
    getEditorContentSnapshotsFromMirrors,
    getEditorContentSnapshotTargetId,
    getOpenDirtyEditorSaveCandidatesFromMirrors,
    getTabDirtyStates,
    markEditorSaveCandidatesClean,
    normalizeEditorPath,
    pruneEditorBufferRegistry,
    shouldFlushEditorContentSnapshotImmediately,
    type EditorBufferRegistry,
} from '../utils/editorBufferRegistry';
const ExplorerPanel = React.lazy(() => import('./ExplorerPanel').then(module => ({ default: module.ExplorerPanel })));
const ChatPanelV3 = React.lazy(() => import('../chat/rendering/ChatPanel').then(module => ({ default: module.ChatPanel })));
const GitPanel = React.lazy(() => import('./GitPanel').then(module => ({ default: module.GitPanel })));
const FileHistoryPanel = React.lazy(() => import('./FileHistoryPanel').then(module => ({ default: module.FileHistoryPanel })));
const DocumentViewer = React.lazy(() => import('./DocumentViewer').then(module => ({ default: module.DocumentViewer })));
const StorageSetupModal = React.lazy(() => import('./StorageSetupModal').then(module => ({ default: module.StorageSetupModal })));
const ProtocolExplorer = React.lazy(() => import('./dev/ProtocolExplorer').then(module => ({ default: module.ProtocolExplorer })));

type ShutdownDecision = 'save' | 'discard' | 'cancel';
type FileChangesDetectedPayload = {
    count: number;
    paths: string[];
};

interface ShutdownPromptState {
    files: string[];
    overflowCount: number;
    resolve: (decision: ShutdownDecision) => void;
}

interface ShutdownSaveErrorState {
    message: string;
    resolve: () => void;
}

function isBoundarySuffixMatch(full: string, suffix: string): boolean {
    if (!full.endsWith(suffix)) return false;
    if (full.length === suffix.length) return true;
    return full[full.length - suffix.length - 1] === '/';
}

function findMatchingChange(
    filePath: string,
    changes: UncommittedChange[],
): UncommittedChange | undefined {
    const target = normalizeEditorPath(filePath);
    const matches = changes
        .filter((change) => {
            const candidate = normalizeEditorPath(change.file_path);
            return candidate === target
                || isBoundarySuffixMatch(candidate, target)
                || isBoundarySuffixMatch(target, candidate);
        })
        .sort((a, b) => b.timestamp - a.timestamp);

    return matches.find((item) => item.unified_diff.trim().length > 0) ?? matches[0];
}

function findMatchingChangeRange(
    filePath: string,
    changes: UncommittedChange[],
): { startLine: number; endLine: number } | null {
    const change = findMatchingChange(filePath, changes);
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

function useNoopChat() {
    const [messages, setMessages] = useState<any[]>([]);
    const [selectedModelId, setSelectedModelId] = useState<string>('disabled');
    const [chatMode, setChatMode] = useState<ChatMode>('planning');
    const [activeTodos, setActiveTodos] = useState<any[]>([]);
    const [messageQueue, setMessageQueue] = useState<any[]>([]);

    const sendMessage = useCallback(() => Promise.resolve(), []);
    const stopGeneration = useCallback(() => Promise.resolve(), []);
    const refreshModels = useCallback(() => Promise.resolve<ModelInfo[]>([]), []);
    const approveTool = useCallback(() => Promise.resolve(), []);
    const approveToolDecision = useCallback(() => Promise.resolve(), []);
    const respondToApprovalRequest = useCallback(() => Promise.resolve(), []);
    const approveSingleCommand = useCallback(() => Promise.resolve(), []);
    const skipSingleCommand = useCallback(() => Promise.resolve(), []);
    const newConversation = useCallback(() => Promise.resolve(), []);
    const undoTool = useCallback(() => Promise.resolve(), []);
    const loadConversation = useCallback(() => Promise.resolve(), []);
    const editLastUserMessage = useCallback(() => Promise.resolve(null), []);
    const deleteQueuedRequest = useCallback((index: number) => {
        setMessageQueue(prev => prev.filter((_, currentIndex) => currentIndex !== index));
    }, []);

    return {
        messages,
        loading: false,
        error: null as string | null,
        sendMessage,
        stopGeneration,
        models: [],
        refreshModels,
        selectedModelId,
        setSelectedModelId,
        chatMode,
        setChatMode,
        pendingActions: null,
        pendingApprovalRequest: null,
        waitingForApproval: false,
        approveTool,
        approveToolDecision,
        respondToApprovalRequest,
        approveSingleCommand,
        skipSingleCommand,
        newConversation,
        undoTool,
        setConversation: setMessages,
        loadConversation,
        toolActivity: null,
        chatActivities: [],
        activeTodos,
        setActiveTodos,
        messageQueue,
        deleteQueuedRequest,
        editLastUserMessage,
    };
}

function useNoopTabManager(_uncommittedChanges: { file_path: string }[]) {
    const [tabs, setTabs] = useState<Tab[]>([]);
    const [activeTabId, setActiveTabId] = useState<string | null>(null);
    const [aiEditedFilePaths, setAiEditedFilePaths] = useState<Set<string>>(new Set());
    const [unseenAiEditedFilePaths, setUnseenAiEditedFilePaths] = useState<Set<string>>(new Set());
    const processingFilesRef = useRef<Set<string>>(new Set());

    const activeTab = useMemo(
        () => tabs.find(tab => tab.id === activeTabId) ?? null,
        [tabs, activeTabId],
    );
    const activeFilename = activeTab?.title ?? null;

    const handleTabClick = useCallback((tabId: string) => {
        setActiveTabId(tabId);
    }, []);

    const handleFileSelect = useCallback((path: string, highlightLines?: { startLine: number; endLine: number } | null) => {
        const tabId = `file-${path}`;
        setTabs(prev => {
            const existingTab = prev.find(tab => tab.type === 'file' && tab.path === path);
            if (existingTab) {
                return highlightLines
                    ? prev.map(tab => tab.id === existingTab.id ? { ...tab, highlightLines: highlightLines ?? undefined } : tab)
                    : prev;
            }

            return [
                ...prev,
                {
                    id: tabId,
                    title: path.split('/').pop() || path,
                    type: 'file',
                    path,
                    highlightLines: highlightLines ?? undefined,
                },
            ];
        });
        setActiveTabId(tabId);
    }, []);

    const handleTabClose = useCallback((tabId: string) => {
        setTabs(prev => prev.filter(tab => tab.id !== tabId));
        setActiveTabId(prev => prev === tabId ? null : prev);
    }, []);

    const handleTabReorder = useCallback((fromIndex: number, toIndex: number) => {
        setTabs(prev => {
            const next = [...prev];
            const [moved] = next.splice(fromIndex, 1);
            if (!moved) {
                return prev;
            }
            next.splice(toIndex, 0, moved);
            return next;
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
            return next;
        });
    }, []);

    const handleTabCloseAll = useCallback(() => {
        setTabs([]);
        setActiveTabId(null);
    }, []);

    const handleTabCloseOthers = useCallback((tabId: string) => {
        setTabs(prev => prev.filter(tab => tab.id === tabId));
        setActiveTabId(tabId);
    }, []);

    const handleEphemeralSave = useCallback(async () => undefined, []);

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
    };
}

// ---------------------------------------------------------------------------
// Debug-flag hook selection.
//
// Each disable* debug flag swaps a subsystem hook for a noop implementation.
// The selection is frozen in a useState initializer on first render — after
// DebugFlagBootstrap has merged the backend runtime flags, so the timing is
// identical to the old useMemo reads — and can never change for the lifetime
// of the mount because the setter is never exposed. The wrapper is called
// unconditionally, so hook order is stable and the Rules of Hooks hold.
// If a flag ever needs to change at runtime, move the branch INSIDE the
// subsystem hook (an `enabled` option); do not branch on it in a component.
// ---------------------------------------------------------------------------

function useNoopChatAdapter(..._args: Parameters<typeof useChatV2>) {
    return useNoopChat();
}

function useChatForLayout(...args: Parameters<typeof useChatV2>) {
    const [useImpl] = useState(() =>
        readDebugFlag('disableChatHook') ? useNoopChatAdapter : useChatV2,
    );
    return useImpl(...args);
}

function useUncommittedChangesDisabled(..._args: Parameters<typeof useUncommittedChanges>) {
    return {
        changes: [],
        acceptFile: async () => false,
        acceptAll: async () => false,
        rejectFile: async () => false,
        rejectAll: async () => false,
    };
}

function useUncommittedChangesForLayout(...args: Parameters<typeof useUncommittedChanges>) {
    const [useImpl] = useState(() =>
        readDebugFlag('disableUncommittedChanges')
            ? useUncommittedChangesDisabled
            : useUncommittedChanges,
    );
    return useImpl(...args);
}

function useGitStatusDisabled(..._args: Parameters<typeof useGitStatus>) {
    return {
        status: null,
        files: [],
        error: null,
        filesError: null,
        lastRefreshedAt: null,
        refresh: async () => undefined,
        stageFile: async (_path: string) => undefined,
        unstageFile: async (_path: string) => undefined,
        stageAll: async () => undefined,
        unstageAll: async () => undefined,
        commit: async (_message: string) => undefined,
        push: async () => undefined,
        diff: async (_path: string, _staged: boolean) => '',
        generateCommitMessage: async (_modelId?: string) => '',
        commitPreflight: async () => ({
            canCommit: false,
            isRepo: false,
            branch: null,
            isDetached: false,
            hasUpstream: false,
            hasConflicts: false,
            stagedCount: 0,
            errorMessage: 'Git status disabled',
            errorKey: 'git_disabled',
        }),
    };
}

function useGitStatusForLayout(...args: Parameters<typeof useGitStatus>) {
    const [useImpl] = useState(() =>
        readDebugFlag('disableGitStatus') ? useGitStatusDisabled : useGitStatus,
    );
    return useImpl(...args);
}

function useTabManagerForLayout(...args: Parameters<typeof useTabManager>) {
    const [useImpl] = useState(() =>
        readDebugFlag('disableTabManager') ? useNoopTabManager : useTabManager,
    );
    return useImpl(...args);
}

function useLayoutEventsDisabled(..._args: Parameters<typeof useLayoutEvents>) {
    return {
        researchProgress: null,
        finalizeResearchActivities: (_loading: boolean, _wasStoppedByUser: boolean) => undefined,
    };
}

function useLayoutEventsForLayout(...args: Parameters<typeof useLayoutEvents>) {
    const [useImpl] = useState(() =>
        readDebugFlag('disableLayoutEvents') ? useLayoutEventsDisabled : useLayoutEvents,
    );
    return useImpl(...args);
}

function useProjectStateDisabled(..._args: Parameters<typeof useProjectState>) {
    return { loaded: true, isClosing: false };
}

function useProjectStateForLayout(...args: Parameters<typeof useProjectState>) {
    const [useImpl] = useState(() =>
        readDebugFlag('disableProjectState') ? useProjectStateDisabled : useProjectState,
    );
    return useImpl(...args);
}

function useWarmupDisabled(..._args: Parameters<typeof useWarmup>) {
    return { trackActivity: () => undefined };
}

function useWarmupForLayout(...args: Parameters<typeof useWarmup>) {
    const [useImpl] = useState(() =>
        readDebugFlag('disableWarmup') ? useWarmupDisabled : useWarmup,
    );
    return useImpl(...args);
}

const AppLayoutInner: React.FC = () => {
    recordDebugPerf('Layout.render');
    const { t } = useTranslation();
    const appWindow = getCurrentWindow();
    const { bootstrap } = useStartupBootstrap();
    const disableTerminalSurface = useMemo(() => readDebugSurfaceFlag('disableTerminal'), []);
    const disableEditorSurface = useMemo(() => readDebugSurfaceFlag('disableEditor'), []);
    const disableChatSurface = useMemo(() => readDebugSurfaceFlag('disableChat'), []);
    const disableActivityBar = useMemo(() => readDebugFlag('disableActivityBar'), []);
    const disableSidebarOverlay = useMemo(() => readDebugFlag('disableSidebarOverlay'), []);
    const disableEditorChrome = useMemo(() => readDebugFlag('disableEditorChrome'), []);
    const disableChatChrome = useMemo(() => readDebugFlag('disableChatChrome'), []);
    const disableEditorWidthObserver = useMemo(() => readDebugFlag('disableEditorWidthObserver'), []);

    // Sidebar State
    const [isSidebarOpen, setIsSidebarOpen] = useState(false);
    const [activeSidebar, setActiveSidebar] = useState<'explorer' | 'git' | 'history'>('explorer');
    const sidebarRef = useRef<HTMLDivElement>(null);
    const activityBarRef = useRef<HTMLDivElement>(null);
    const [shutdownPrompt, setShutdownPrompt] = useState<ShutdownPromptState | null>(null);
    const [shutdownSaveError, setShutdownSaveError] = useState<ShutdownSaveErrorState | null>(null);
    const [autoApproveRunCommands, setAutoApproveRunCommands] = useState(false);
    const [indexHealth, setIndexHealth] = useState<IndexHealthSnapshot | null>(null);
    const isGitSidebarVisible = isSidebarOpen && activeSidebar === 'git';

    // Track window maximized and fullscreen state for resize borders
    const [isMaximized, setIsMaximized] = useState(false);
    const [isFullscreen, setIsFullscreen] = useState(false);

    useEffect(() => {
        let unlisten: (() => void) | undefined;
        const setupListener = async () => {
            setIsMaximized(await appWindow.isMaximized());
            setIsFullscreen(await appWindow.isFullscreen());
            unlisten = await appWindow.onResized(async () => {
                setIsMaximized(await appWindow.isMaximized());
                setIsFullscreen(await appWindow.isFullscreen());
            });
        };
        setupListener();
        return () => {
            if (unlisten) unlisten();
        };
    }, [appWindow]);

    const handleResizeMouseDown = useCallback((
        e: React.MouseEvent,
        direction: 'East' | 'North' | 'NorthEast' | 'NorthWest' | 'South' | 'SouthEast' | 'SouthWest' | 'West'
    ) => {
        if (e.button !== 0) return; // Only drag-resize on primary click
        e.preventDefault();
        e.stopPropagation();
        appWindow.startResizeDragging(direction).catch((err) => {
            console.error('[Layout] Failed to start resize dragging:', err);
        });
    }, [appWindow]);

    // Latest dirty active-file buffer content, read at chat-send time so each
    // turn's workspace payload can be hashed from the same buffer warmup uses.
    // Kept in a ref because useChatV2 runs before the editor buffer registry is
    // set up below.
    const activeBufferContentForChatRef = useRef<string | null>(null);
    const getActiveBufferContentForChat = useCallback(
        () => activeBufferContentForChatRef.current,
        [],
    );
    const chat = useChatForLayout({ autoApproveRunCommands, getActiveBufferContent: getActiveBufferContentForChat });
    const [wasStoppedByUser, setWasStoppedByUser] = useState(false);
    const {
        changes: uncommittedChanges,
        acceptFile: acceptFileChange,
        acceptAll: acceptAllChanges,
        rejectFile: rejectFileChange,
        rejectAll: rejectAllChanges,
    } = useUncommittedChangesForLayout();
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
    } = useGitStatusForLayout({ includeFiles: isGitSidebarVisible });
    const gitChangedCount = gitStatus?.changedCount ?? 0;
    const {
        selectedModelId,
        setSelectedModelId,
        refreshModels,
        stopGeneration: stopChatGeneration,
        sendMessage: sendChatMessage,
    } = chat;
    const terminalPaneRef = useRef<TerminalPaneHandle>(null);

    const handleStopGeneration = useCallback(async () => {
        setWasStoppedByUser(true);
        await stopChatGeneration();
    }, [stopChatGeneration]);

    // Tab management (CRUD, history, keyboard shortcuts, backend sync)
    const tabManager = useTabManagerForLayout(uncommittedChanges);
    const {
        tabs, setTabs, activeTabId, setActiveTabId, activeTab, activeFilename,
        aiEditedFilePaths, setAiEditedFilePaths,
        unseenAiEditedFilePaths, setUnseenAiEditedFilePaths,
        processingFilesRef, handleTabClick, handleFileSelect, handleTabClose,
        handleTabReorder, handleTabMoveToBeginning, handleTabMoveToEnd,
        handleTabCloseAll, handleTabCloseOthers, handleEphemeralSave,
    } = tabManager;

    // Sync active tab and open file paths to EditorContext
    const { setActiveFile, setOpenFiles, getEditorSnapshot } = useEditorActions();
    const activeFilePath = activeTab?.path ?? null;
    const openFilePathsJson = useMemo(
        () => JSON.stringify(
            tabs
                .filter(t => t.type === 'file' && t.path)
                .map(t => t.path!),
        ),
        [tabs],
    );
    const pendingTabContentStateRef = useRef<{
        tabId: string;
        state: EditorContentState;
    } | null>(null);
    const pendingTabContentTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const activeContentSnapshotRef = useRef<(() => EditorContentState) | null>(null);
    const editorBufferRegistryRef = useRef<EditorBufferRegistry>({});
    const tabDirtyStates = useMemo(() => getTabDirtyStates(editorBufferRegistryRef.current, tabs), [tabs]);
    const appBarTabs = useMemo(() => tabs.map((tab) => ({
        id: tab.id,
        title: tab.title,
        path: tab.path,
        isEphemeral: tab.type === 'ephemeral',
        isDirty: tabDirtyStates.get(tab.id) ?? false,
        isDeletedOnDisk: Boolean(tab.isDeletedOnDisk),
        hasVirtualChanges: Boolean(tab.path && uncommittedChanges.some(change => change.file_path === tab.path)),
        isAiEdited: Boolean(tab.path && aiEditedFilePaths.has(tab.path)),
        hasUnreadAiEdit: Boolean(tab.path && unseenAiEditedFilePaths.has(tab.path)),
    })), [tabs, tabDirtyStates, uncommittedChanges, aiEditedFilePaths, unseenAiEditedFilePaths]);

    const requestShutdownDecision = useCallback((files: string[], overflowCount: number) => (
        new Promise<ShutdownDecision>((resolve) => {
            setShutdownPrompt({ files, overflowCount, resolve });
        })
    ), []);

    useEffect(() => {
        const fileTabs = tabs.filter((tab) => tab.type === 'file' && tab.path);
        if (fileTabs.length === 0) {
            return;
        }

        let disposed = false;

        const refreshOpenFilePresence = async (changedPaths?: string[]) => {
            const changedPathSet = changedPaths?.map(normalizeEditorPath);
            const tabsToCheck = changedPathSet
                ? fileTabs.filter((tab) => {
                    const tabPath = normalizeEditorPath(tab.path!);
                    return changedPathSet.some((changedPath) => (
                        tabPath === changedPath
                        || tabPath.endsWith(`/${changedPath}`)
                        || changedPath.endsWith(`/${tabPath}`)
                    ));
                })
                : fileTabs;

            if (tabsToCheck.length === 0) {
                return;
            }

            const results = await Promise.all(tabsToCheck.map(async (tab) => {
                try {
                    return [tab.id, await exists(tab.path!)] as const;
                } catch (error) {
                    console.warn('[Layout] Failed to check open file presence:', tab.path, error);
                    return [tab.id, true] as const;
                }
            }));

            if (disposed) {
                return;
            }

            const existsByTabId = new Map(results);
            setTabs((prev) => prev.map((tab) => {
                if (!existsByTabId.has(tab.id)) {
                    return tab;
                }

                const nextDeletedOnDisk = !existsByTabId.get(tab.id);
                if (Boolean(tab.isDeletedOnDisk) === nextDeletedOnDisk) {
                    return tab;
                }

                return {
                    ...tab,
                    isDeletedOnDisk: nextDeletedOnDisk,
                };
            }));
        };

        const unlistenPromise = listen<FileChangesDetectedPayload>('file-changes-detected', (event) => {
            void refreshOpenFilePresence(event.payload.paths);
        });

        return () => {
            disposed = true;
            unlistenPromise.then((unlisten) => unlisten());
        };
    }, [setTabs, tabs]);

    const requestShutdownSaveErrorAck = useCallback((message: string) => (
        new Promise<void>((resolve) => {
            setShutdownSaveError({ message, resolve });
        })
    ), []);

    const handleShutdownPromptDecision = useCallback((decision: ShutdownDecision) => {
        setShutdownPrompt((current) => {
            current?.resolve(decision);
            return null;
        });
    }, []);

    const handleShutdownSaveErrorAck = useCallback(() => {
        setShutdownSaveError((current) => {
            current?.resolve();
            return null;
        });
    }, []);

    useEffect(() => {
        if (!shutdownPrompt && !shutdownSaveError) {
            return;
        }

        const handleKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                if (shutdownSaveError) {
                    handleShutdownSaveErrorAck();
                    return;
                }
                handleShutdownPromptDecision('cancel');
            }
        };

        document.addEventListener('keydown', handleKeyDown);
        return () => document.removeEventListener('keydown', handleKeyDown);
    }, [handleShutdownPromptDecision, handleShutdownSaveErrorAck, shutdownPrompt, shutdownSaveError]);

    useEffect(() => {
        setActiveFile(activeFilePath);
    }, [activeFilePath, setActiveFile]);

    useEffect(() => {
        setOpenFiles(JSON.parse(openFilePathsJson) as string[]);
    }, [openFilePathsJson, setOpenFiles]);

    useEffect(() => {
        return subscribeBladeNestedEventType('Language', 'IndexStatus', (payload) => {
            setIndexHealth(payload.health);
        });
    }, []);

    const handleOpenChatFile = useCallback((path: string) => {
        const highlightLines = findMatchingChangeRange(path, uncommittedChanges);
        handleFileSelect(path, highlightLines);
    }, [handleFileSelect, uncommittedChanges]);

    const handleExplorerFileSelect = useCallback((path: string, _line?: number, _character?: number) => {
        handleFileSelect(path);
    }, [handleFileSelect]);

    const handleImplementPlan = useCallback((planText: string) => {
        const trimmedPlan = planText.trim();
        if (!trimmedPlan) {
            return;
        }

        chat.setChatMode('code');
        chat.sendMessage(
            `${t('chat.implementPlanMessage')}\n\n${trimmedPlan}`,
            undefined,
            undefined,
            'code'
        );
    }, [chat, t]);

    const handleOpenProject = useCallback(async () => {
        try {
            const selected = await open({
                directory: true,
                multiple: false,
                title: t('fileTree.openProject'),
            });
            if (!selected || Array.isArray(selected)) {
                return;
            }

            await invoke('open_workspace', { path: selected });
            setWorkspacePath(selected);
            setProjectId(await invoke<string | null>('get_project_id', { workspacePath: selected }));
            setShowStorageSetup(false);
            setTabs([]);
            setActiveTabId(null);
            editorBufferRegistryRef.current = {};
            pendingTabContentStateRef.current = null;
            if (pendingTabContentTimerRef.current) {
                clearTimeout(pendingTabContentTimerRef.current);
                pendingTabContentTimerRef.current = null;
            }
            setActiveSidebar('explorer');
            setIsSidebarOpen(true);
        } catch (error) {
            console.error('[Layout] Failed to open project:', error);
        }
    }, [setActiveTabId, setTabs, t]);

    const flushPendingTabContentState = useCallback((tabId?: string | null) => {
        const pending = pendingTabContentStateRef.current;
        if (!pending) {
            return;
        }
        if (tabId && pending.tabId !== tabId) {
            return;
        }

        if (pendingTabContentTimerRef.current) {
            clearTimeout(pendingTabContentTimerRef.current);
            pendingTabContentTimerRef.current = null;
        }

        pendingTabContentStateRef.current = null;
        editorBufferRegistryRef.current = applyEditorContentSnapshot(editorBufferRegistryRef.current, pending.state);

        setTabs(prev => {
            const targetTabId = getEditorContentSnapshotTargetId(
                prev.filter(tab => tab.type === 'file'),
                pending.tabId,
                pending.state,
            );
            if (!targetTabId) {
                return prev;
            }

            return prev.map(tab => {
                if (tab.id !== targetTabId) {
                    return tab;
                }

                return applyEditorContentSnapshotToMirror(tab, pending.state, {
                    mirrorDraftContent: false,
                    mirrorDirtySavedContent: false,
                });
            });
        });
    }, [setTabs]);

    useEffect(() => {
        return () => {
            flushPendingTabContentState(activeTabId);
        };
    }, [activeTabId, flushPendingTabContentState]);

    const handleActiveTabContentStateChange = useCallback((state: EditorContentState) => {
        if (!activeTabId) return;

        const targetTabId = getEditorContentSnapshotTargetId(
            tabs.filter(tab => tab.type === 'file'),
            activeTabId,
            state,
        );
        if (!targetTabId) return;
        editorBufferRegistryRef.current = applyEditorContentSnapshot(editorBufferRegistryRef.current, state);

        pendingTabContentStateRef.current = {
            tabId: targetTabId,
            state,
        };

        if (pendingTabContentTimerRef.current) {
            clearTimeout(pendingTabContentTimerRef.current);
        }

        if (shouldFlushEditorContentSnapshotImmediately(state)) {
            flushPendingTabContentState(targetTabId);
            return;
        }

        pendingTabContentTimerRef.current = setTimeout(() => {
            flushPendingTabContentState(targetTabId);
        }, 120);
    }, [activeTabId, flushPendingTabContentState, tabs]);

    useEffect(() => {
        editorBufferRegistryRef.current = pruneEditorBufferRegistry(
            editorBufferRegistryRef.current,
            (JSON.parse(openFilePathsJson) as string[]),
        );
    }, [openFilePathsJson]);

    useEffect(() => {
        editorBufferRegistryRef.current = applyEditorContentSnapshots(
            editorBufferRegistryRef.current,
            getEditorContentSnapshotsFromMirrors(
                editorBufferRegistryRef.current,
                tabs.filter(tab => tab.type === 'file'),
                activeTabId,
            ),
        );
    }, [activeTabId, tabs]);

    const handleRegisterContentSnapshot = useCallback((getSnapshot: (() => EditorContentState) | null) => {
        activeContentSnapshotRef.current = getSnapshot;
    }, []);

    // Tauri event listeners (file open, research progress, change-applied, etc.)
    const { researchProgress, finalizeResearchActivities } = useLayoutEventsForLayout({
            setTabs,
            setActiveTabId,
            setAiEditedFilePaths,
            setUnseenAiEditedFilePaths,
            processingFilesRef,
            setConversation: chat.setConversation,
        });

    // Settings modal state
    const [isSettingsOpen, setIsSettingsOpen] = useState(false);
    const [initialSettingsSection, setInitialSettingsSection] = useState<import('./SettingsModal').SettingsSection | undefined>(undefined);

    // Listen for open-settings custom event (from WelcomePage or ChatPanel)
    useEffect(() => {
        const handleOpenSettings = (event: Event) => {
            const customEvent = event as CustomEvent<{ section?: import('./SettingsModal').SettingsSection }>;
            setInitialSettingsSection(customEvent.detail?.section);
            setIsSettingsOpen(true);
        };
        document.addEventListener('open-settings', handleOpenSettings);
        return () => document.removeEventListener('open-settings', handleOpenSettings);
    }, []);

    const openConfigurationSettings = useCallback(() => {
        setInitialSettingsSection('configuration');
        setIsSettingsOpen(true);
    }, []);

    const openDefaultSettings = useCallback(() => {
        setInitialSettingsSection(undefined);
        setIsSettingsOpen(true);
    }, []);

    // Listen for telegram_command events
    useEffect(() => {
        const unlistenPromise = listen<string>('telegram_command', (event) => {
            const command = event.payload;
            console.debug('[Layout] Received telegram_command:', command);
            sendChatMessage(command);
        });

        return () => {
            unlistenPromise.then(unlisten => unlisten());
        };
    }, [sendChatMessage]);

    // First-time setup modal state (RFC-002)
    const [showStorageSetup, setShowStorageSetup] = useState(false);
    const [workspacePath, setWorkspacePath] = useState<string | null>(null);
    const [projectId, setProjectId] = useState<string | null>(null);
    const bootstrapAppliedRef = useRef(false);
    const bootstrapHasApiKey = useMemo(() => {
        const apiKey = bootstrap?.remote_settings.api_key;
        return apiKey ? apiKey.length > 0 : null;
    }, [bootstrap]);

    const projectName = useMemo(() => {
        if (!workspacePath) return null;
        const normalized = workspacePath.replace(/[/\\]+$/, '');
        const segments = normalized.split(/[/\\]/).filter(Boolean);
        return segments.length > 0 ? segments[segments.length - 1] : null;
    }, [workspacePath]);

    useEffect(() => {
        const titleParts = [t('app.name')];
        if (projectName) titleParts.push(projectName);
        if (activeFilename) titleParts.push(activeFilename);
        const title = titleParts.join(' - ');
        invoke('set_window_title', { title }).catch((err) => {
            console.error('[Layout] Failed to set native window title:', err);
            appWindow.setTitle(title).catch((fallbackErr) => {
                console.error('[Layout] Failed to set window title:', fallbackErr);
            });
        });
        document.title = title;
    }, [appWindow, projectName, activeFilename, t]);

    useEffect(() => {
        if (!bootstrap || bootstrapAppliedRef.current) return;

        bootstrapAppliedRef.current = true;
        setWorkspacePath(bootstrap.core_state.workspace.path);
        setProjectId(bootstrap.core_state.workspace.project_id);

        if (bootstrap.has_zblade_directory !== null) {
            const exists = bootstrap.has_zblade_directory;
            if (!exists) {
                setShowStorageSetup(true);
            }
        } else {
            setShowStorageSetup(false);
        }
    }, [bootstrap]);

    useEffect(() => {
        if (!workspacePath) {
            setAutoApproveRunCommands(false);
            return;
        }

        let disposed = false;
        let unlistenProjectSettings: (() => void) | undefined;

        const loadAutoApproveRunCommands = async () => {
            try {
                const settings = await invoke<BackendSettings>('load_project_settings', {
                    projectPath: workspacePath,
                });
                if (!disposed) {
                    setAutoApproveRunCommands(settings.auto_approve_run_commands === true);
                }
            } catch (error) {
                if (!disposed) {
                    setAutoApproveRunCommands(false);
                }
                console.debug('[Layout] Failed to load auto-approve run command setting:', error);
            }
        };

        void loadAutoApproveRunCommands();
        void listen('project-settings-changed', () => {
            void loadAutoApproveRunCommands();
        }).then((unlisten) => {
            if (disposed) {
                unlisten();
                return;
            }
            unlistenProjectSettings = unlisten;
        });

        return () => {
            disposed = true;
            unlistenProjectSettings?.();
        };
    }, [workspacePath]);

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
    const activeEditorContentState = activeTab?.type === 'file'
        ? getEditorContentSnapshotForMirror(editorBufferRegistryRef.current, activeTab)
        : null;
    // Publish the dirty buffer for the chat send path (see ref declaration
    // above). Clean/unavailable → null so the backend hashes disk content.
    activeBufferContentForChatRef.current = activeEditorContentState?.isDirty
        ? (activeEditorContentState.draftContent ?? null)
        : null;
    const activeUncommittedChange = activeTab?.type === 'file' && activeTab.path
        ? findMatchingChange(activeTab.path, uncommittedChanges)
        : undefined;

    const handleBeforeShutdown = useCallback(async () => {
        const activeSnapshot = activeContentSnapshotRef.current?.();
        let registryForShutdown = editorBufferRegistryRef.current;

        if (activeSnapshot) {
            registryForShutdown = applyEditorContentSnapshot(registryForShutdown, activeSnapshot);
        }

        const pending = pendingTabContentStateRef.current;
        if (pending) {
            registryForShutdown = applyEditorContentSnapshot(registryForShutdown, pending.state);
        }
        editorBufferRegistryRef.current = registryForShutdown;

        const dirtySaveCandidates = getOpenDirtyEditorSaveCandidatesFromMirrors(
            registryForShutdown,
            tabs.filter(tab => tab.type === 'file'),
        );

        if (dirtySaveCandidates.length === 0) {
            return true;
        }

        const filesForPrompt = getDirtyEditorSaveCandidateDisplayNames(
            dirtySaveCandidates.slice(0, 5),
            tabs.filter(tab => tab.type === 'file'),
            t('editor.untitled'),
        );
        const decision = await requestShutdownDecision(filesForPrompt, Math.max(0, dirtySaveCandidates.length - filesForPrompt.length));

        if (decision === 'cancel') {
            return false;
        }

        if (decision === 'discard') {
            return true;
        }

        try {
            await Promise.all(dirtySaveCandidates.map(candidate => invoke('write_file_content', {
                path: candidate.path,
                content: candidate.draft,
            })));
            editorBufferRegistryRef.current = markEditorSaveCandidatesClean(
                editorBufferRegistryRef.current,
                dirtySaveCandidates,
            );
            pendingTabContentStateRef.current = null;
            if (pendingTabContentTimerRef.current) {
                clearTimeout(pendingTabContentTimerRef.current);
                pendingTabContentTimerRef.current = null;
            }
            return true;
        } catch (error) {
            console.error('[Layout] Failed to save dirty files before shutdown:', error);
            const errorMessage = error instanceof Error ? error.message : String(error);
            await requestShutdownSaveErrorAck(errorMessage);
            return false;
        }
    }, [requestShutdownDecision, requestShutdownSaveErrorAck, t, tabs]);

    // Project state persistence
    const { loaded: stateLoaded, isClosing } = useProjectStateForLayout({
            projectPath: workspacePath,
            tabs: tabs.map(t => ({ id: t.id, title: t.title, type: t.type, path: t.path })),
            activeTabId,
            selectedModelId,
            terminals: terminalState.terminals,
            activeTerminalId: terminalState.activeId,
            terminalHeight,
            chatPanelWidth,
            onStateLoaded: handleStateLoaded,
            beforeShutdown: handleBeforeShutdown,
        });

    // Cache warmup (Blade Protocol v2.1 + context prefetch)
    // Automatically warms cache on launch, model change, workspace change,
    // active-file change, and save — carrying an editor/workspace context
    // bundle so zcoderd can prewarm the provider prompt cache.
    // Wait for stateLoaded to prevent multiple warmups during initialization
    const activeFileDirtyForWarmup = Boolean(activeEditorContentState?.isDirty);
    const getWarmupEditorSnapshot = useCallback((): EditorWarmupSnapshot => {
        const editorSnap = getEditorSnapshot();
        const openFiles = tabs
            .filter((tab) => tab.type === 'file' && tab.path)
            .map((tab, index) => ({
                path: tab.path!,
                isModified: tabDirtyStates.get(tab.id) ?? false,
                tabOrder: index,
            }));
        const contentState = activeTab?.type === 'file'
            ? getEditorContentSnapshotForMirror(editorBufferRegistryRef.current, activeTab)
            : null;
        return {
            activeFile: activeFilePath,
            cursor: editorSnap.cursorLine != null && editorSnap.cursorColumn != null
                ? { line: editorSnap.cursorLine, column: editorSnap.cursorColumn }
                : null,
            selection: editorSnap.selectionStartLine != null && editorSnap.selectionEndLine != null
                ? {
                    start: { line: editorSnap.selectionStartLine, column: 1 },
                    end: { line: editorSnap.selectionEndLine, column: 1 },
                }
                : null,
            visibleRange: null, // not tracked in frontend state yet
            openFiles,
            activeBufferContent: contentState?.isDirty
                ? contentState.draftContent ?? null
                : null,
        };
    }, [tabs, tabDirtyStates, activeTab, activeFilePath, getEditorSnapshot]);
    // Return value intentionally unused: the hook runs for its warmup effects.
    useWarmupForLayout(workspacePath, selectedModelId, stateLoaded, {
            activeFilePath,
            activeFileDirty: activeFileDirtyForWarmup,
            getSnapshot: getWarmupEditorSnapshot,
        });

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
        if (disableEditorWidthObserver) {
            setEditorColumnWidth(undefined);
            return;
        }

        const el = editorColumnRef.current;
        if (!el) return;
        const ro = new ResizeObserver(entries => {
            recordDebugPerf('Layout.editorWidthObserver');
            setEditorColumnWidth(entries[0].contentRect.width);
        });
        ro.observe(el);
        return () => ro.disconnect();
    }, [disableEditorWidthObserver]);

    // Toggle sidebar function
    const toggleSidebar = (view: 'explorer' | 'git' | 'history') => {
        if (isSidebarOpen && activeSidebar === view) {
            setIsSidebarOpen(false);
        } else {
            setActiveSidebar(view);
            setIsSidebarOpen(true);
        }
    };

    // Close the sidebar when clicking anywhere outside the sidebar panel and activity bar.
    // Ignores clicks inside the context-menu portal so that right-click actions
    // (e.g. creating a new file/folder) don't close the sidebar before the
    // menu item's onClick fires.
    useEffect(() => {
        if (!isSidebarOpen) return;
        const handlePointerDown = (event: PointerEvent) => {
            const target = event.target as Node | null;
            if (!target) return;
            if (sidebarRef.current?.contains(target)) return;
            if (activityBarRef.current?.contains(target)) return;
            if (target instanceof Element && target.closest('[data-context-menu]')) return;
            setIsSidebarOpen(false);
        };
        document.addEventListener('pointerdown', handlePointerDown, true);
        return () => document.removeEventListener('pointerdown', handlePointerDown, true);
    }, [isSidebarOpen]);

    const editorViewportBottomInset = !disableTerminalSurface && terminalHeight > 0 ? terminalHeight : 0;
    const desktopShellV1 = readDebugFlag('desktopShellV1');
    const shellGap = desktopShellV1 ? '0px' : 'var(--panel-gap)';
    const activityBarStyle: React.CSSProperties = desktopShellV1 ? {
        width: '46px',
        backgroundColor: 'var(--surface-toolbar)',
        borderRadius: '0',
        borderRight: '1px solid var(--separator-default)',
        boxShadow: 'var(--shadow-persistent)',
    } : {
        width: '46px',
        backgroundColor: 'var(--bg-panel)',
        borderRadius: 'var(--panel-radius)',
        border: '1px solid var(--border-default)',
        boxShadow: 'var(--panel-shadow)',
    };
    const sidebarStyle: React.CSSProperties = desktopShellV1 ? {
        left: disableActivityBar ? '0px' : '46px',
        borderRadius: '0',
        borderRight: '1px solid var(--separator-default)',
        boxShadow: 'var(--shadow-persistent)',
        zIndex: 200,
        transform: isSidebarOpen ? 'translateX(0)' : 'translateX(-100%)',
    } : {
        left: disableActivityBar ? 'var(--panel-gap)' : 'calc(46px + 2 * var(--panel-gap))',
        borderRadius: 'var(--panel-radius)',
        border: '1px solid var(--border-default)',
        boxShadow: isSidebarOpen ? 'var(--panel-shadow)' : 'none',
        zIndex: 200,
        transform: isSidebarOpen ? 'translateX(0)' : 'translateX(-100%)',
    };
    const editorChromeStyle: React.CSSProperties | undefined = disableEditorChrome ? undefined : desktopShellV1 ? {
        backgroundColor: 'var(--surface-editor)',
        borderRadius: '0',
        borderRight: '1px solid var(--separator-default)',
        boxShadow: 'var(--shadow-persistent)',
    } : {
        backgroundColor: 'var(--bg-editor)',
        borderRadius: 'var(--panel-radius)',
        border: '1px solid var(--border-default)',
        boxShadow: 'var(--panel-shadow)',
    };
    const terminalDrawerStyle: React.CSSProperties = {
        height: terminalHeight,
        backgroundColor: 'var(--term-bg)',
        borderRadius: disableEditorChrome || desktopShellV1 ? '0' : `0 0 var(--panel-radius) var(--panel-radius)`,
        boxShadow: disableEditorChrome || desktopShellV1 ? 'none' : '0 -10px 28px rgba(0,0,0,0.45)',
        transform: terminalHeight > 0 ? 'translateY(0)' : 'translateY(100%)',
        transition: isTerminalDragging ? 'none' : 'transform var(--transition-base)',
    };
    const chatChromeStyle: React.CSSProperties = disableChatChrome ? {
        width: chatPanelWidth,
    } : desktopShellV1 ? {
        width: chatPanelWidth,
        backgroundColor: 'var(--surface-pane)',
        borderRadius: '0',
        borderLeft: '1px solid var(--separator-default)',
        boxShadow: 'var(--shadow-persistent)',
    } : {
        width: chatPanelWidth,
        backgroundColor: 'var(--bg-panel)',
        borderRadius: 'var(--panel-radius)',
        border: '1px solid var(--border-default)',
        boxShadow: 'var(--panel-shadow)',
    };
    const statusBarStyle: React.CSSProperties = desktopShellV1 ? {
        height: '24px',
        backgroundColor: 'var(--surface-toolbar)',
        margin: '0',
        borderRadius: '0',
        borderTop: '1px solid var(--separator-default)',
        boxShadow: 'var(--shadow-persistent)',
    } : {
        height: '24px',
        backgroundColor: 'var(--bg-panel)',
        margin: '0 var(--panel-gap) var(--panel-gap)',
        borderRadius: 'var(--panel-radius)',
        border: '1px solid var(--border-default)',
        boxShadow: 'var(--panel-shadow)',
    };

    return (
        <div className="h-screen w-screen bg-(--bg-app) overflow-hidden flex flex-col relative font-sans text-(--fg-primary)">
            {/* Window Resize Handles for Frameless/Undecorated Window */}
            {!isMaximized && !isFullscreen && (
                <>
                    {/* Corners */}
                    <div
                        className="absolute top-0 left-0 w-2 h-2 z-[9999] cursor-nw-resize select-none"
                        onMouseDown={(e) => handleResizeMouseDown(e, 'NorthWest')}
                    />
                    <div
                        className="absolute top-0 right-0 w-2 h-2 z-[9999] cursor-ne-resize select-none"
                        onMouseDown={(e) => handleResizeMouseDown(e, 'NorthEast')}
                    />
                    <div
                        className="absolute bottom-0 left-0 w-2 h-2 z-[9999] cursor-sw-resize select-none"
                        onMouseDown={(e) => handleResizeMouseDown(e, 'SouthWest')}
                    />
                    <div
                        className="absolute bottom-0 right-0 w-2 h-2 z-[9999] cursor-se-resize select-none"
                        onMouseDown={(e) => handleResizeMouseDown(e, 'SouthEast')}
                    />

                    {/* Borders */}
                    <div
                        className="absolute top-0 left-2 right-2 h-1 z-[9998] cursor-n-resize select-none"
                        onMouseDown={(e) => handleResizeMouseDown(e, 'North')}
                    />
                    <div
                        className="absolute bottom-0 left-2 right-2 h-1 z-[9998] cursor-s-resize select-none"
                        onMouseDown={(e) => handleResizeMouseDown(e, 'South')}
                    />
                    <div
                        className="absolute top-2 bottom-2 left-0 w-1 z-[9998] cursor-w-resize select-none"
                        onMouseDown={(e) => handleResizeMouseDown(e, 'West')}
                    />
                    <div
                        className="absolute top-2 bottom-2 right-0 w-1 z-[9998] cursor-e-resize select-none"
                        onMouseDown={(e) => handleResizeMouseDown(e, 'East')}
                    />
                </>
            )}

            {/* Unified App Bar: title bar + tab strip merged */}
            <AppBar
                tabs={appBarTabs}
                activeTabId={activeTabId}
                projectName={projectName}
                onTabClick={handleTabClick}
                onTabClose={handleTabClose}
                onReorder={handleTabReorder}
                onTabMoveToBeginning={handleTabMoveToBeginning}
                onTabMoveToEnd={handleTabMoveToEnd}
                onTabCloseAll={handleTabCloseAll}
                onTabCloseOthers={handleTabCloseOthers}
                tabStripMaxWidth={editorColumnWidth}
                chatPanelWidth={chatPanelWidth}
                chatVisible={!disableChatSurface}
                onOpenProject={handleOpenProject}
            />

            <div
                className="flex-1 flex overflow-hidden relative"
                style={{ padding: shellGap, gap: shellGap }}
            >
                {/* Activity Bar (Vertical) — floating pill */}
                {!disableActivityBar && (
                    <div
                        ref={activityBarRef}
                        className="flex flex-col items-center py-4 gap-6 z-50 shrink-0 relative"
                        style={activityBarStyle}
                    >
                    <button
                        type="button"
                        onClick={() => toggleSidebar('explorer')}
                        title={t('activityBar.explorer')}
                        aria-label={t('activityBar.explorer')}
                        aria-pressed={isSidebarOpen && activeSidebar === 'explorer'}
                        className={`relative appearance-none border-0 p-2 rounded-[calc(var(--panel-radius)*0.45)] cursor-pointer transition-all duration-(--transition-fast) ${isSidebarOpen && activeSidebar === 'explorer'
                            ? 'text-(--fg-primary) bg-(--bg-surface)'
                            : 'text-(--fg-nav) hover:text-(--fg-primary) hover:bg-(--bg-surface)'}
                        `}
                    >
                        {isSidebarOpen && activeSidebar === 'explorer' && (
                            <div className="absolute left-0 top-0 bottom-0 w-[2px] bg-(--accent-ai) rounded-r" />
                        )}
                        <svg aria-hidden="true" className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                        </svg>
                    </button>
                    <button
                        type="button"
                        onClick={() => toggleSidebar('git')}
                        title={t('activityBar.git')}
                        aria-label={t('activityBar.git')}
                        aria-pressed={isSidebarOpen && activeSidebar === 'git'}
                        className={`relative appearance-none border-0 p-2 rounded-[calc(var(--panel-radius)*0.45)] cursor-pointer transition-all duration-(--transition-fast) ${isSidebarOpen && activeSidebar === 'git'
                            ? 'text-(--fg-primary) bg-(--bg-surface)'
                            : 'text-(--fg-nav) hover:text-(--fg-primary) hover:bg-(--bg-surface)'}
                        `}
                    >
                        {isSidebarOpen && activeSidebar === 'git' && (
                            <div className="absolute left-0 top-0 bottom-0 w-[2px] bg-(--accent-ai) rounded-r" />
                        )}
                        <GitBranch className="w-5 h-5" aria-hidden="true" />
                        {gitStatus?.isRepo && gitChangedCount > 0 && (
                            <span className="absolute -bottom-1 -right-1 min-w-[14px] h-3 px-1 rounded-full bg-(--accent-ai) text-[9px] leading-3 text-(--fg-bright) text-center shadow-sm">
                                {Math.min(gitChangedCount, 99)}
                            </span>
                        )}
                    </button>
                    <button
                        type="button"
                        onClick={() => toggleSidebar('history')}
                        title={t('activityBar.fileHistory')}
                        aria-label={t('activityBar.fileHistory')}
                        aria-pressed={isSidebarOpen && activeSidebar === 'history'}
                        className={`relative appearance-none border-0 p-2 rounded-[calc(var(--panel-radius)*0.45)] cursor-pointer transition-all duration-(--transition-fast) ${isSidebarOpen && activeSidebar === 'history'
                            ? 'text-(--fg-primary) bg-(--bg-surface)'
                            : 'text-(--fg-nav) hover:text-(--fg-primary) hover:bg-(--bg-surface)'}
                        `}
                    >
                        {isSidebarOpen && activeSidebar === 'history' && (
                            <div className="absolute left-0 top-0 bottom-0 w-[2px] bg-(--accent-ai) rounded-r" />
                        )}
                        <Clock className="w-5 h-5" aria-hidden="true" />
                    </button>
                    <button
                        type="button"
                        disabled
                        title={t('activityBar.searchComingSoon')}
                        aria-label={t('activityBar.search')}
                        className="hidden relative appearance-none border-0 bg-transparent p-2 rounded-[calc(var(--panel-radius)*0.45)] text-(--fg-nav) opacity-40 cursor-not-allowed transition-all duration-(--transition-fast)"
                    >
                        <svg aria-hidden="true" className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                        </svg>
                    </button>
                    <button
                        type="button"
                        onMouseDown={(event) => {
                            if (event.button !== 0) return;
                            event.preventDefault();
                            event.stopPropagation();
                            openDefaultSettings();
                        }}
                        onClick={(event) => {
                            event.preventDefault();
                            event.stopPropagation();
                            openDefaultSettings();
                        }}
                        title={t('activityBar.settings')}
                        aria-label={t('activityBar.settings')}
                        className="relative mt-auto appearance-none border-0 bg-transparent p-2 rounded-[calc(var(--panel-radius)*0.45)] text-(--fg-nav) hover:text-(--fg-primary) hover:bg-(--bg-surface) transition-all duration-(--transition-fast) cursor-pointer"
                    >
                        <Settings className="w-5 h-5" aria-hidden="true" />
                    </button>
                    </div>
                )}

                {/* Explorer / Sidebar (Floating overlay — anchored to outer row, above all editor content) */}
                {!disableSidebarOverlay && (
                    <div
                        ref={sidebarRef}
                        className={`
                            absolute top-0 bottom-0 w-80 bg-(--bg-panel) flex flex-col overflow-hidden
                            transition-all duration-(--transition-fast) ease-[cubic-bezier(0.22,1,0.36,1)]
                            ${isSidebarOpen
                                ? 'opacity-100 visible'
                                : 'opacity-0 invisible pointer-events-none'}
                        `}
                        style={sidebarStyle}
                    >
                    {activeSidebar === 'explorer' && (
                        <Suspense fallback={<div className="h-full flex items-center justify-center text-(--fg-tertiary)">{t('sidebar.loadingExplorer', 'Loading explorer...')}</div>}>
                            <ExplorerPanel onFileSelect={handleExplorerFileSelect} activeFile={tabs.find(t => t.id === activeTabId)?.path || null} />
                        </Suspense>
                    )}
                    {activeSidebar === 'git' && (
                        <Suspense fallback={<div className="h-full flex items-center justify-center text-(--fg-tertiary)">{t('sidebar.loadingGit')}</div>}>
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
                                onGenerateCommitMessage={async () => {
                                    const hasValidModel = chat.models.some(
                                        (m) => m.id === selectedModelId || m.api_id === selectedModelId
                                    );
                                    if (!hasValidModel) {
                                        throw new Error(t('errors.noModelSelected'));
                                    }
                                    return generateGitCommitMessage(selectedModelId);
                                }}
                                onCommitPreflight={commitPreflightGit}
                            />
                        </Suspense>
                    )}
                    {activeSidebar === 'history' && (
                        <Suspense fallback={<div className="h-full flex items-center justify-center text-(--fg-tertiary)">{t('sidebar.loadingHistory')}</div>}>
                            <FileHistoryPanel activeFile={tabs.find(t => t.id === activeTabId)?.path || null} />
                        </Suspense>
                    )}
                    </div>
                )}

                {/* Content Area */}
                <div className="flex-1 flex min-w-0 overflow-hidden" style={{ gap: shellGap }}>

                    {/* Editor & Terminal — floating card */}
                    <div
                        ref={editorColumnRef}
                        className="flex-1 flex flex-col min-w-0 relative overflow-hidden"
                        style={editorChromeStyle}
                    >

                        <div
                            className="flex-1 overflow-hidden relative"
                        >
                            {!disableEditorSurface && activeTab?.type === 'file' && (
                                <div
                                    className="absolute inset-x-0 top-0 z-10"
                                    style={{ bottom: editorViewportBottomInset }}
                                >
                                    <EditorPanel
                                        activeFile={activeTab.path || null}
                                        highlightLines={activeTab.highlightLines || null}
                                        workspaceRoot={workspacePath}
                                        hasRemoteApiKey={bootstrapHasApiKey}
                                        savedContent={activeEditorContentState?.savedContent ?? null}
                                        draftContent={activeEditorContentState?.draftContent ?? null}
                                        isDirty={Boolean(activeEditorContentState?.isDirty)}
                                        isDeletedOnDisk={Boolean(activeEditorContentState?.isDeletedOnDisk)}
                                        uncommittedChange={activeUncommittedChange}
                                        onAcceptFileChange={acceptFileChange}
                                        onRejectFileChange={rejectFileChange}
                                        onContentStateChange={handleActiveTabContentStateChange}
                                        onRegisterContentSnapshot={handleRegisterContentSnapshot}
                                        onOpenSettings={() => setIsSettingsOpen(true)}
                                        onOpenProject={handleOpenProject}
                                    />
                                </div>
                            )}

                            {!disableEditorSurface && tabs.length === 0 && (
                                <div className="absolute inset-x-0 top-0 z-10" style={{ bottom: editorViewportBottomInset }}>
                                    <EditorPanel
                                        activeFile={null}
                                        workspaceRoot={workspacePath}
                                        hasRemoteApiKey={bootstrapHasApiKey}
                                        onOpenSettings={() => setIsSettingsOpen(true)}
                                        onOpenProject={handleOpenProject}
                                    />
                                </div>
                            )}

                            {!disableEditorSurface && activeTab?.type === 'ephemeral' && (
                                <div
                                    key={activeTab.id}
                                    className="absolute inset-x-0 top-0 z-10"
                                    style={{ bottom: editorViewportBottomInset }}
                                >
                                    <Suspense fallback={<div className="h-full w-full bg-(--bg-editor)" />}>
                                        <DocumentViewer
                                            documentId={activeTab.id}
                                            title={activeTab.title}
                                            content={activeTab.content || ''}
                                            isEphemeral={true}
                                            suggestedName={activeTab.suggestedName}
                                            onClose={() => handleTabClose(activeTab.id)}
                                            onSave={(savedPath) => handleEphemeralSave(activeTab.id, savedPath)}
                                            onImplementPlan={handleImplementPlan}
                                        />
                                    </Suspense>
                                </div>
                            )}

                            {disableEditorSurface && (
                                <div
                                    className="absolute inset-0 flex items-center justify-center text-sm text-(--fg-secondary)"
                                    style={{ bottom: editorViewportBottomInset }}
                                >
                                    Editor surface disabled via `?disableEditor=1`
                                </div>
                            )}
                        </div>

                        {/* Terminal Drawer — floats over the editor from the bottom */}
                        {!disableTerminalSurface && (
                            <div
                                className="absolute bottom-0 left-0 right-0 z-20 flex flex-col overflow-hidden"
                                style={terminalDrawerStyle}
                            >
                                {/* Drag handle strip */}
                                <div
                                    className="relative h-3 shrink-0 group flex items-center resize-y-cursor select-none"
                                    onMouseDown={handleTerminalMouseDown}
                                >
                                    <div
                                        className={`w-full transition-all duration-(--transition-fast) ${isTerminalDragging
                                            ? 'h-[2px] bg-(--accent-ai)'
                                            : 'h-px bg-(--border-subtle) group-hover:h-[2px] group-hover:bg-(--accent-ai)'
                                        }`}
                                    />
                                </div>
                                <div className="flex-1 min-h-0 overflow-hidden">
                                    <TerminalPane ref={terminalPaneRef} workspaceRoot={workspacePath} />
                                </div>
                            </div>
                        )}
                    </div>

                    {/* Chat Panel Resizer */}
                    {!disableChatSurface && !disableChatChrome && (
                        <div
                            className="relative w-0 z-40 shrink-0 select-none pointer-events-none"
                            style={{
                                marginLeft: 'calc(var(--panel-gap) / -2)',
                                marginRight: 'calc(var(--panel-gap) / -2)',
                            }}
                        >
                            <div
                                className={`absolute inset-y-0 left-1/2 -translate-x-1/2 flex items-center justify-center pointer-events-auto resize-x-cursor transition-colors duration-(--transition-fast) ${isChatDragging
                                    ? 'bg-(--accent-ai)/10'
                                    : 'bg-transparent hover:bg-(--accent-ai)/10'
                                    }`}
                                style={{ width: 'var(--panel-gap)' }}
                                onMouseDown={handleChatMouseDown}
                            >
                                <div className={`h-full w-px ${isChatDragging ? 'bg-(--accent-ai)' : 'bg-transparent hover:bg-(--accent-ai)'}`} />
                            </div>
                        </div>
                    )}

                    {/* AI Chat — floating card */}
                    {!disableChatSurface && (
                        <div
                            style={chatChromeStyle}
                            className="min-w-[280px] max-w-[800px] flex flex-col z-30 overflow-hidden"
                        >
                            <Suspense fallback={<div className="flex-1 bg-(--bg-panel) h-full w-full" />}>
                                <ChatPanelV3
                                    messages={chat.messages}
                                    loading={chat.loading}
                                    error={chat.error}
                                    sendMessage={chat.sendMessage}
                                    stopGeneration={handleStopGeneration}
                                    models={chat.models}
                                    selectedModelId={chat.selectedModelId}
                                    setSelectedModelId={chat.setSelectedModelId}
                                    chatMode={chat.chatMode}
                                    setChatMode={chat.setChatMode}
                                    pendingActions={chat.pendingActions}
                                    pendingApprovalRequest={chat.pendingApprovalRequest}
                                    waitingForApproval={chat.waitingForApproval}
                                    approveToolDecision={chat.approveToolDecision}
                                    respondToApprovalRequest={chat.respondToApprovalRequest}
                                    approveSingleCommand={chat.approveSingleCommand}
                                    skipSingleCommand={chat.skipSingleCommand}
                                    projectId={projectId || "default-project"}
                                    onLoadConversation={chat.loadConversation}
                                    onPrependConversation={(messages) => {
                                        chat.setConversation((current: ChatMessage[]) => [...messages, ...current]);
                                    }}
                                    researchProgress={researchProgress}
                                    onNewConversation={chat.newConversation}
                                    onUndoTool={chat.undoTool}
                                    onOpenFile={handleOpenChatFile}
                                    workspaceRoot={workspacePath}
                                    uncommittedChanges={uncommittedChanges}
                                    onAcceptAllChanges={acceptAllChanges}
                                    onRejectAllChanges={rejectAllChanges}
                                    toolActivity={chat.toolActivity}
                                    chatActivities={chat.chatActivities}
                                    activeTodos={chat.activeTodos}
                                    queuedRequests={chat.messageQueue}
                                    deleteQueuedRequest={chat.deleteQueuedRequest}
                                    editLastUserMessage={chat.editLastUserMessage}
                                    onImplementPlan={handleImplementPlan}
                                />
                            </Suspense>
                        </div>
                    )}

                </div>
            </div>

            {/* Status Bar */}
            <div
                className="text-(--fg-tertiary) flex items-center px-3 text-[10px] font-mono justify-between select-none z-40"
                style={statusBarStyle}
            >
                <div className="flex items-center gap-1.5">
                    <span className="flex items-center gap-1.5 hover:text-(--fg-secondary) cursor-pointer transition-colors duration-(--transition-fast)">
                        <GitBranch className="w-3 h-3" aria-hidden="true" />
                        {gitStatus?.branch ?? t('statusBar.noBranch')}{gitStatus?.dirty ? '*' : ''}
                    </span>
                    {shouldShowIndexStatusCue(indexHealth) && (
                        <span
                            className={`inline-flex max-w-[42vw] items-center gap-1 font-normal ${indexHealth.status === 'error' ? 'text-(--state-danger)' : ''}`}
                            title={formatIndexStatusTitle(indexHealth)}
                        >
                            {indexHealth.status === 'error' ? (
                                <AlertTriangle className="h-3 w-3 shrink-0" aria-hidden="true" />
                            ) : indexHealth.status === 'partial' || indexHealth.status === 'stale' ? (
                                <Clock className="h-3 w-3 shrink-0" aria-hidden="true" />
                            ) : (
                                <Loader2 className="h-3 w-3 shrink-0 animate-spin" aria-hidden="true" />
                            )}
                            <span className="truncate">
                                {formatIndexStatusLabel(indexHealth, t)}
                            </span>
                        </span>
                    )}
                </div>
                <div className="flex items-center gap-4 opacity-70">
                    {autoApproveRunCommands && (
                        <button
                            type="button"
                            onClick={openConfigurationSettings}
                            className="yolo-pill inline-flex items-center gap-1 rounded-[calc(var(--panel-radius)*0.35)] border border-[color-mix(in_srgb,var(--accent-warning)_42%,transparent)] bg-[color-mix(in_srgb,var(--accent-warning)_12%,transparent)] px-1.5 py-0.5 font-semibold text-(--accent-warning) opacity-100 transition-colors hover:bg-[color-mix(in_srgb,var(--accent-warning)_18%,transparent)]"
                        >
                            <AlertTriangle className="h-3 w-3" aria-hidden="true" />
                            {t('statusBar.yoloMode')}
                        </button>
                    )}
                    {(disableEditorSurface || disableTerminalSurface || disableChatSurface) && (
                        <span className="text-(--accent-warning)">
                            debug:
                            {disableEditorSurface ? ' editor-off' : ''}
                            {disableTerminalSurface ? ' terminal-off' : ''}
                            {disableChatSurface ? ' chat-off' : ''}
                        </span>
                    )}
                    {/* Saving Indicator */}
                    {isClosing && (
                        <span className="text-(--accent-secondary) animate-pulse font-semibold">{t('statusBar.saving')}</span>
                    )}
                    <span>{t('editor.encoding')}</span>
                    <span>{(() => {
                        const activeTab = tabs.find(tab => tab.id === activeTabId);
                        if (!activeTab?.path) return null;
                        const ext = activeTab.path.split('.').pop()?.toLowerCase();
                        const langMap: Record<string, string> = {
                            rs: 'Rust', ts: 'TypeScript', tsx: 'TypeScript React', js: 'JavaScript', jsx: 'JavaScript React',
                            py: 'Python', rb: 'Ruby', go: 'Go', java: 'Java', kt: 'Kotlin', swift: 'Swift',
                            c: 'C', cpp: 'C++', h: t('statusBar.languages.cHeader'), hpp: t('statusBar.languages.cppHeader'), cs: 'C#',
                            html: 'HTML', css: 'CSS', scss: 'SCSS', less: 'Less', json: 'JSON', yaml: 'YAML', yml: 'YAML',
                            xml: 'XML', md: 'Markdown', toml: 'TOML', sql: 'SQL', sh: t('statusBar.languages.shell'), bash: 'Bash',
                            lua: 'Lua', zig: 'Zig', dart: 'Dart', php: 'PHP', r: 'R',
                            svelte: 'Svelte', vue: 'Vue', astro: 'Astro',
                            txt: t('statusBar.languages.plainText'), csv: 'CSV', svg: 'SVG',
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

            {shutdownPrompt && (
                <div className="fixed inset-0 z-9999 flex items-center justify-center p-4 animate-in fade-in duration-(--transition-fast)">
                    <button
                        type="button"
                        aria-label={t('common.cancel')}
                        className="absolute inset-0 bg-(--glass-bg) backdrop-blur-sm"
                        onClick={() => handleShutdownPromptDecision('cancel')}
                    />
                    <div
                        role="dialog"
                        aria-modal="true"
                        aria-labelledby="shutdown-unsaved-title"
                        className="relative w-full max-w-lg overflow-hidden rounded-(--panel-radius) border border-(--border-focus) bg-(--bg-panel) shadow-(--shadow-xl) animate-in fade-in zoom-in-95 duration-(--transition-base)"
                    >
                        <div className="flex items-start gap-3 border-b border-(--border-subtle) bg-(--bg-surface)/70 px-5 py-4">
                            <div className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-[calc(var(--panel-radius)*0.7)] border border-[color-mix(in_srgb,var(--accent-warning)_38%,transparent)] bg-[color-mix(in_srgb,var(--accent-warning)_14%,transparent)] text-(--accent-warning)">
                                <AlertTriangle className="h-5 w-5" aria-hidden="true" />
                            </div>
                            <div className="min-w-0">
                                <h2 id="shutdown-unsaved-title" className="text-base font-semibold text-(--fg-primary)">
                                    {t('editor.unsavedChanges')}
                                </h2>
                                <p className="mt-1 text-sm leading-5 text-(--fg-secondary)">
                                    {t('editor.shutdownUnsaved.summary', {
                                        count: shutdownPrompt.files.length + shutdownPrompt.overflowCount,
                                        fileWord: t((shutdownPrompt.files.length + shutdownPrompt.overflowCount) === 1
                                            ? 'editor.shutdownUnsaved.fileSingular'
                                            : 'editor.shutdownUnsaved.filePlural'),
                                    })}
                                </p>
                            </div>
                        </div>
                        <div className="px-5 py-4">
                            <div className="rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-app)/60 p-3">
                                <ul className="space-y-1.5">
                                    {shutdownPrompt.files.map((file) => (
                                        <li key={file} className="flex items-center gap-2 text-sm text-(--fg-primary)">
                                            <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-(--accent-warning)" />
                                            <span className="min-w-0 truncate font-mono text-xs">{file}</span>
                                        </li>
                                    ))}
                                    {shutdownPrompt.overflowCount > 0 && (
                                        <li className="pl-3.5 text-xs text-(--fg-tertiary)">
                                            {t('editor.shutdownUnsaved.overflowPlain', { count: shutdownPrompt.overflowCount })}
                                        </li>
                                    )}
                                </ul>
                            </div>
                            <p className="mt-4 text-sm text-(--fg-secondary)">
                                {t('editor.shutdownUnsaved.question')}
                            </p>
                        </div>
                        <div className="flex flex-col-reverse gap-2 border-t border-(--border-subtle) bg-(--bg-surface)/40 px-5 py-4 sm:flex-row sm:justify-end">
                            <button
                                type="button"
                                onClick={() => handleShutdownPromptDecision('cancel')}
                                className="rounded-[calc(var(--panel-radius)*0.55)] px-3 py-2 text-xs font-medium text-(--fg-secondary) transition-all duration-(--transition-fast) hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)"
                            >
                                {t('common.cancel')}
                            </button>
                            <button
                                type="button"
                                onClick={() => handleShutdownPromptDecision('discard')}
                                className="rounded-[calc(var(--panel-radius)*0.55)] px-3 py-2 text-xs font-medium text-(--fg-secondary) transition-all duration-(--transition-fast) hover:bg-[color-mix(in_srgb,var(--state-danger)_14%,transparent)] hover:text-(--state-danger)"
                            >
                                {t('editor.shutdownUnsaved.dontSave')}
                            </button>
                            <button
                                type="button"
                                onClick={() => handleShutdownPromptDecision('save')}
                                className="rounded-[calc(var(--panel-radius)*0.55)] bg-(--accent-ai) px-3 py-2 text-xs font-semibold text-(--fg-bright) shadow-(--shadow-sm) transition-all duration-(--transition-fast) hover:brightness-110"
                            >
                                {t('common.save')}
                            </button>
                        </div>
                    </div>
                </div>
            )}

            {shutdownSaveError && (
                <div className="fixed inset-0 z-9999 flex items-center justify-center p-4 animate-in fade-in duration-(--transition-fast)">
                    <button
                        type="button"
                        aria-label={t('common.ok')}
                        className="absolute inset-0 bg-(--glass-bg) backdrop-blur-sm"
                        onClick={handleShutdownSaveErrorAck}
                    />
                    <div
                        role="alertdialog"
                        aria-modal="true"
                        aria-labelledby="shutdown-save-error-title"
                        className="relative w-full max-w-md overflow-hidden rounded-(--panel-radius) border border-[color-mix(in_srgb,var(--state-danger)_42%,var(--border-focus))] bg-(--bg-panel) shadow-(--shadow-xl) animate-in fade-in zoom-in-95 duration-(--transition-base)"
                    >
                        <div className="flex items-start gap-3 border-b border-(--border-subtle) bg-(--bg-surface)/70 px-5 py-4">
                            <div className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-[calc(var(--panel-radius)*0.7)] border border-[color-mix(in_srgb,var(--state-danger)_38%,transparent)] bg-[color-mix(in_srgb,var(--state-danger)_14%,transparent)] text-(--state-danger)">
                                <AlertTriangle className="h-5 w-5" aria-hidden="true" />
                            </div>
                            <div className="min-w-0">
                                <h2 id="shutdown-save-error-title" className="text-base font-semibold text-(--fg-primary)">
                                    {t('editor.saveFailed')}
                                </h2>
                                <p className="mt-1 text-sm leading-5 text-(--fg-secondary)">
                                    {t('editor.shutdownUnsaved.saveFailedSummary')}
                                </p>
                            </div>
                        </div>
                        <div className="px-5 py-4">
                            <div className="max-h-40 overflow-auto rounded-[calc(var(--panel-radius)*0.65)] border border-(--border-subtle) bg-(--bg-app)/60 p-3 font-mono text-xs text-(--fg-secondary)">
                                {shutdownSaveError.message}
                            </div>
                        </div>
                        <div className="flex justify-end border-t border-(--border-subtle) bg-(--bg-surface)/40 px-5 py-4">
                            <button
                                type="button"
                                onClick={handleShutdownSaveErrorAck}
                                className="rounded-[calc(var(--panel-radius)*0.55)] bg-(--accent-ai) px-3 py-2 text-xs font-semibold text-(--fg-bright) shadow-(--shadow-sm) transition-all duration-(--transition-fast) hover:brightness-110"
                            >
                                {t('common.ok')}
                            </button>
                        </div>
                    </div>
                </div>
            )}

            {/* Settings Modal */}
            {isSettingsOpen && (
                <SettingsModalLazy
                    isOpen={isSettingsOpen}
                    onClose={() => setIsSettingsOpen(false)}
                    initialSection={initialSettingsSection}
                    workspacePath={workspacePath}
                    onRefreshModels={refreshModels}
                />
            )}

            {/* First-time Storage Setup Modal (RFC-002) */}
            {workspacePath && (
                <Suspense fallback={null}>
                    <StorageSetupModal
                        isOpen={showStorageSetup}
                        workspacePath={workspacePath}
                        onComplete={() => setShowStorageSetup(false)}
                    />
                </Suspense>
            )}
        </div>
    );
};

const MinimalShellLayout: React.FC = () => {
    return (
        <div className="h-screen w-screen bg-(--bg-app) overflow-hidden flex flex-col font-sans text-(--fg-primary)">
            <AppBar tabs={[]} activeTabId={null} projectName="Debug Shell" />
            <div className="flex-1 flex items-center justify-center text-sm text-(--fg-secondary)">
                Minimal layout mode enabled
            </div>
            <div
                className="text-(--fg-tertiary) flex items-center px-3 text-[10px] font-mono justify-between select-none z-40"
                style={{
                    height: '24px',
                    backgroundColor: 'var(--bg-panel)',
                    margin: '0 var(--panel-gap) var(--panel-gap)',
                    borderRadius: 'var(--panel-radius)',
                    border: '1px solid var(--border-default)',
                    boxShadow: 'var(--panel-shadow)',
                }}
            >
                <div className="flex items-center gap-1.5">
                    <span>minimal-shell</span>
                </div>
                <div className="flex items-center gap-4 opacity-70">
                    <span>debug</span>
                </div>
            </div>
        </div>
    );
};

export const AppLayout: React.FC = () => {
    const minimalLayout = readDebugFlag('minimalLayout');

    return (
        <EditorProvider>
            {minimalLayout ? <MinimalShellLayout /> : <AppLayoutInner />}
        </EditorProvider>
    );
};
