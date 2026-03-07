import React, { useEffect, useRef, useCallback, useState, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { ArrowDown, Check, X, Settings, Key, Loader2, ChevronDown, ChevronRight } from 'lucide-react';
import { useCommandExecution } from '../hooks/useCommandExecution';
import { useHistory } from '../hooks/useHistory';
import type { ChatMessage as ChatMessageType, ComposerMention, ImageAttachment, ModelInfo, QueuedRequest, ToolActivityState } from '../types/chat';

import type { StructuredAction, TodoItem } from '../types/events';
import type { RemoteAiConfig } from '../types/settings';
import { ChatMessage } from './ChatMessage';
import { ChatTabBar } from './ChatTabBar';
import { CommandCenter } from './CommandCenter';
import { HistoryTab } from './HistoryTab';
import { ProgressIndicator } from './ProgressIndicator';
import { GlobalChangeActions } from './editor/GlobalChangeActions';
import { TaskPanel } from './TaskPanel';
import { QueuePanel } from './QueuePanel';
import type { UncommittedChange } from '../types/uncommitted';
import { deriveChatRows } from '../utils/chatTimeline';

interface ResearchProgress {
    message: string;
    stage: string;
    percent: number;
    isActive: boolean;
}

interface ChatPanelProps {
    messages: ChatMessageType[];
    loading: boolean;
    error: string | null;
    sendMessage: (text: string, attachments?: ImageAttachment[], mentions?: ComposerMention[]) => void;
    stopGeneration: () => void;
    models: ModelInfo[];
    selectedModelId: string;
    setSelectedModelId: (modelId: string) => void;
    pendingActions: StructuredAction[] | null;
    approveToolDecision: (decision: string) => void;
    skipSingleCommand: (callId: string) => void;
    projectId: string;
    onLoadConversation: (messages: ChatMessageType[]) => void;
    researchProgress?: ResearchProgress | null;
    onNewConversation: () => void;
    onUndoTool: (toolCallId: string) => void;
    onOpenFile: (path: string) => void;
    uncommittedChanges: UncommittedChange[];
    onAcceptAllChanges: () => void;
    onRejectAllChanges: () => void;
    toolActivity?: ToolActivityState | null;
    activeTodos: TodoItem[];
    queuedRequests: QueuedRequest[];
    deleteQueuedRequest: (index: number) => void;
}

const pendingResponseWords = [
    'Working...',
    'Thinking...',
    'Waiting...',
    'Ruminating...',
] as const;

const PendingResponseIndicator: React.FC = () => {
    const [wordIndex, setWordIndex] = useState(0);

    useEffect(() => {
        const intervalId = window.setInterval(() => {
            setWordIndex((prev) => (prev + 1) % pendingResponseWords.length);
        }, 1800);

        return () => window.clearInterval(intervalId);
    }, []);

    return (
        <div className="px-4 py-3">
            <div className="inline-flex items-center gap-3 rounded-2xl border border-emerald-500/15 bg-[linear-gradient(180deg,rgba(16,185,129,0.08),rgba(24,24,27,0.7))] px-4 py-3 text-[11px] font-medium text-(--fg-secondary) shadow-[0_16px_40px_rgba(0,0,0,0.18)] backdrop-blur-md">
                <div className="flex h-8 w-8 items-center justify-center rounded-2xl border border-emerald-500/20 bg-emerald-500/10">
                    <Loader2 className="h-4 w-4 animate-spin text-emerald-300" />
                </div>
                <div className="flex flex-col items-start">
                    <span className="text-[10px] font-semibold uppercase tracking-[0.18em] text-emerald-300/80">
                        Assistant is responding
                    </span>
                    <span key={pendingResponseWords[wordIndex]} className="pending-response-word text-sm font-semibold text-(--fg-primary)">
                        {pendingResponseWords[wordIndex]}
                    </span>
                </div>
                <div className="ml-1 flex items-center gap-1">
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-400/80 animate-pulse" />
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-400/60 animate-pulse [animation-delay:180ms]" />
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-400/40 animate-pulse [animation-delay:360ms]" />
                </div>
            </div>
        </div>
    );
};

const ChatPanelComponent: React.FC<ChatPanelProps> = ({
    messages,
    loading,
    error,
    sendMessage,
    stopGeneration,
    models,
    selectedModelId,
    setSelectedModelId,
    pendingActions,
    approveToolDecision,
    skipSingleCommand,
    projectId,
    onLoadConversation,
    researchProgress,
    onNewConversation,
    onUndoTool,
    onOpenFile,
    uncommittedChanges,
    onAcceptAllChanges,
    onRejectAllChanges,
    toolActivity,
    activeTodos,
    queuedRequests,
    deleteQueuedRequest,
}) => {
    const { t } = useTranslation();
    const { stopCommandExecution } = useCommandExecution();
    const [taskPanelCollapsed, setTaskPanelCollapsed] = useState(false);
    const { loadConversation } = useHistory();
    const isUserAtBottomRef = useRef(true);
    const prevMessageCountRef = useRef(0);
    const [activeTab, setActiveTab] = useState<'chat' | 'history'>('chat');
    const [hasApiKey, setHasApiKey] = useState<boolean>(true);
    const [composerPrefill, setComposerPrefill] = useState<QueuedRequest | null>(null);
    const [showScrollToBottom, setShowScrollToBottom] = useState(false);
    const [isToolActivityExpanded, setIsToolActivityExpanded] = useState(false);
    const showScrollToBottomRef = useRef(false);

    // Check API Key
    const checkApiKey = useCallback(async () => {
        try {
            const config = await invoke<RemoteAiConfig>('get_remote_ai_settings');
            setHasApiKey(!!config.api_key && config.api_key.length > 0);
        } catch (e) {
            console.error('Failed to check API key:', e);
        }
    }, []);

    useEffect(() => {
        checkApiKey();
        const unlistenPromise = listen('global-settings-changed', checkApiKey);
        return () => {
            unlistenPromise.then(unlisten => unlisten());
        };
    }, [checkApiKey]);

    const scrollContainerRef = useRef<HTMLDivElement>(null);

    const messageCount = messages.length;
    const firstMessageId = messages[0]?.id;
    const lastMessage = messages[messages.length - 1];
    const lastMessageId = lastMessage?.id;
    const lastMessageRole = lastMessage?.role;
    const lastMessageContent = lastMessage?.content ?? '';
    const lastMessageReasoning = lastMessage?.reasoning ?? '';
    const lastMessageBlockCount = lastMessage?.blocks?.length ?? 0;
    const shouldShowPendingResponseIndicator = loading && lastMessage?.role !== 'Assistant';
    const toolActivityKey = toolActivity
        ? `${toolActivity.toolCallId || `${toolActivity.toolName}:${toolActivity.filePath}`}:${toolActivity.action}`
        : null;
    const chatRows = useMemo(() => deriveChatRows(messages, loading, pendingActions), [loading, messages, pendingActions]);
    const streamingSignature = useMemo(() => {
        if (!lastMessage) return '';
        return [
            lastMessageId ?? messageCount,
            lastMessageContent,
            lastMessageReasoning,
            lastMessageBlockCount,
        ].join('|');
    }, [lastMessageBlockCount, lastMessageContent, lastMessageId, lastMessageReasoning, messageCount]);

    useEffect(() => {
        setIsToolActivityExpanded(false);
    }, [toolActivityKey]);

    const scrollToBottom = useCallback(() => {
        const container = scrollContainerRef.current;
        if (!container) return;
        container.scrollTop = container.scrollHeight;
        isUserAtBottomRef.current = true;
        if (showScrollToBottomRef.current) {
            showScrollToBottomRef.current = false;
            setShowScrollToBottom(false);
        }
    }, []);

    // Scroll to bottom when a different conversation is loaded (or on initial mount).
    // Detect conversation change by tracking the first message's ID.
    const prevFirstMsgIdRef = useRef<string | undefined>(undefined);
    useEffect(() => {
        if (messageCount > 0 && firstMessageId !== prevFirstMsgIdRef.current) {
            prevFirstMsgIdRef.current = firstMessageId;
            isUserAtBottomRef.current = true;
            if (showScrollToBottomRef.current) {
                showScrollToBottomRef.current = false;
                setShowScrollToBottom(false);
            }
            prevMessageCountRef.current = messageCount;
            const timer = setTimeout(scrollToBottom, 50);
            return () => clearTimeout(timer);
        }
    }, [firstMessageId, messageCount, scrollToBottom]);

    // Scroll when a new message is appended (or user sends a message)
    useEffect(() => {
        const currentCount = messageCount;

        if (currentCount === prevMessageCountRef.current) {
            return;
        }

        prevMessageCountRef.current = currentCount;
        const justSent = lastMessageRole === 'User';

        if (justSent || isUserAtBottomRef.current) {
            const rafId = requestAnimationFrame(scrollToBottom);
            return () => cancelAnimationFrame(rafId);
        }

        return;
    }, [lastMessageRole, messageCount, scrollToBottom]);

    // Scroll during streaming updates only while user stays at bottom
    useEffect(() => {
        if (!loading || !isUserAtBottomRef.current) return;
        const rafId = requestAnimationFrame(scrollToBottom);
        return () => cancelAnimationFrame(rafId);
    }, [loading, streamingSignature, scrollToBottom]);

    // Prevent default context menu on empty areas
    const handleContextMenu = useCallback((e: React.MouseEvent) => {
        // Always prevent default to avoid native Tauri menu
        e.preventDefault();
    }, []);

    const handleScroll = useCallback((e: React.UIEvent<HTMLDivElement>) => {
        const target = e.target as HTMLDivElement;
        const isBottom = Math.abs(target.scrollHeight - target.scrollTop - target.clientHeight) < 100;
        isUserAtBottomRef.current = isBottom;
        const nextShowScrollToBottom = !isBottom && messageCount > 0;
        if (showScrollToBottomRef.current !== nextShowScrollToBottom) {
            showScrollToBottomRef.current = nextShowScrollToBottom;
            setShowScrollToBottom(nextShowScrollToBottom);
        }
    }, [messageCount]);

    const handleNewConversation = () => {
        onNewConversation();
        setActiveTab('chat');
    };

    const handleSelectConversation = useCallback(async (sessionId: string) => {
        try {
            const conversationMessages = await loadConversation(sessionId);
            onLoadConversation(conversationMessages);
            setActiveTab('chat');
        } catch (e) {
            console.error('Failed to load conversation:', e);
        }
    }, [loadConversation, onLoadConversation]);

    // Stable callback references for ChatMessage - prevents re-renders
    const handleApproveCommand = useCallback(() => {
        approveToolDecision('approve_once');
    }, [approveToolDecision]);

    const handleSkipCommand = useCallback(() => {
        approveToolDecision('reject');
    }, [approveToolDecision]);

    // Individual command approval/skip handlers
    const handleApproveSingleCommand = useCallback((callId: string) => {
        invoke('approve_single_command', { callId, approved: true });
    }, []);

    const handleSkipSingleCommand = useCallback((callId: string) => {
        skipSingleCommand(callId);
    }, [skipSingleCommand]);

    const handleStopCommand = useCallback((callId: string) => {
        stopCommandExecution(callId);
    }, [stopCommandExecution]);

    const handleEditQueuedRequest = useCallback((index: number) => {
        const request = queuedRequests[index];
        if (!request) return;
        setComposerPrefill(request);
        deleteQueuedRequest(index);
    }, [queuedRequests, deleteQueuedRequest]);

    const handleComposerPrefillConsumed = useCallback(() => {
        setComposerPrefill(null);
    }, []);

    return (
        <div className="flex flex-col h-full bg-[var(--bg-app)] text-[var(--fg-primary)] font-sans tracking-tight" onContextMenu={handleContextMenu}>
            {/* Tab Bar */}
            <ChatTabBar
                activeTab={activeTab}
                onTabChange={setActiveTab}
                onNewConversation={handleNewConversation}
            />

            {/* Content Area - conditionally render based on active tab */}
            {activeTab === 'chat' ? (
                <div
                    ref={scrollContainerRef}
                    className="relative flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-zinc-800 scrollbar-track-transparent"
                    onScroll={handleScroll}
                >
                    <div className="mx-auto flex max-w-4xl flex-col gap-1 px-3 py-5 md:px-4">
                        {messages.length === 0 && (
                            <div className="mx-4 mt-10 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]/70 px-6 py-8 text-center shadow-[0_24px_80px_rgba(0,0,0,0.25)] backdrop-blur-md">
                                <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-2xl border border-emerald-500/20 bg-emerald-500/10 text-2xl shadow-[0_0_40px_rgba(16,185,129,0.15)]">
                                    🗡️
                                </div>
                                <h2 className="text-sm font-semibold uppercase tracking-[0.24em] text-[var(--fg-secondary)]">Zaguán Blade</h2>
                                <p className="mx-auto mt-3 max-w-md text-sm leading-6 text-[var(--fg-tertiary)]">
                                    Ask for a fix, paste a task, or describe what you want to build. The assistant can inspect the files you have open, explain unfamiliar code, suggest edits, and help you move from idea to finished change without losing the thread.
                                </p>
                                <div className="mt-5 flex flex-wrap items-center justify-center gap-2 text-[11px] text-[var(--fg-tertiary)]">
                                    <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--bg-app)] px-3 py-1">Understand this file</span>
                                    <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--bg-app)] px-3 py-1">Plan the next change</span>
                                    <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--bg-app)] px-3 py-1">Review command output</span>
                                    <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--bg-app)] px-3 py-1">Attach screenshots</span>
                                </div>
                                <p className="mt-5 text-xs font-mono uppercase tracking-[0.2em] text-[var(--fg-tertiary)]/70">
                                    Start with a goal, question, or bug
                                </p>
                            </div>
                        )}

                        {chatRows.map((row) => (
                            <ChatMessage
                                key={row.key}
                                message={row.message}
                                pendingActions={row.pendingActions}
                                onApproveCommand={row.pendingActions ? handleApproveCommand : undefined}
                                onSkipCommand={row.pendingActions ? handleSkipCommand : undefined}
                                onApproveSingleCommand={row.pendingActions ? handleApproveSingleCommand : undefined}
                                onSkipSingleCommand={row.pendingActions ? handleSkipSingleCommand : undefined}
                                isContinued={row.isContinued}
                                isActive={row.isActive}
                                onUndoTool={onUndoTool}
                                onStopCommand={handleStopCommand}
                                onOpenFile={onOpenFile}
                            />
                        ))}

                        {shouldShowPendingResponseIndicator && <PendingResponseIndicator />}

                        {/* Research progress indicator */}
                        {loading && researchProgress?.isActive && (
                            <div className="px-4">
                                <ProgressIndicator progress={researchProgress} />
                            </div>
                        )}

                        {/* Tool activity indicator - shows streaming tool progress, styled like ToolCallDisplay */}
                        {toolActivity && (() => {
                            const prettyToolNames: Record<string, string> = {
                                'write_file': 'Writing File',
                                'read_file': 'Reading File',
                                'apply_patch': 'Applying Code Changes',
                                'create_file': 'Creating File',
                                'edit_file': 'Editing File',
                                'delete_file': 'Deleting File',
                                'execute_command': 'Running Command',
                                'run_command': 'Running Command',
                                'search_files': 'Searching Code',
                                'list_files': 'Listing Files',
                                'grep_search': 'Searching Code',
                                'find_by_name': 'Finding Files',
                                'multi_edit': 'Multi-Edit File',
                                'list_dir': 'Listing Directory',
                                'list_directory': 'Listing Directory',
                                'codebase_search': 'Searching Codebase',
                                'get_workspace_structure': 'Analyzing Workspace',
                                'view_file': 'Viewing File',
                                'replace_file_content': 'Replacing Content',
                                'multi_replace_file_content': 'Multi-Edit File',
                                'write_to_file': 'Writing to File',
                            };
                            const writingTools = new Set([
                                'write_file',
                                'apply_patch',
                                'create_file',
                                'edit_file',
                                'delete_file',
                                'multi_edit',
                                'replace_file_content',
                                'multi_replace_file_content',
                                'write_to_file',
                            ]);
                            const isWriteTool = writingTools.has(toolActivity.toolName);
                            const isStreaming = toolActivity.action === 'streaming';
                            if (!isWriteTool || !isStreaming) {
                                return null;
                            }

                            const prettyName = prettyToolNames[toolActivity.toolName] || toolActivity.toolName;
                            const displayPath = toolActivity.filePath.split('/').pop() || toolActivity.filePath;
                            const elapsedSeconds = Math.max(0, (toolActivity.lastChunkAt - toolActivity.startedAt) / 1000);
                            const detailItems = [
                                { label: 'Path', value: toolActivity.filePath },
                                { label: 'State', value: 'Streaming file changes' },
                                { label: 'Duration', value: `${elapsedSeconds.toFixed(1)}s` },
                                toolActivity.toolCallId ? { label: 'Call ID', value: toolActivity.toolCallId } : null,
                            ].filter((item): item is { label: string; value: string } => !!item);

                            return (
                                <div className="px-4">
                                    <div className="inline-flex min-w-[320px] max-w-full flex-col overflow-hidden rounded-xl border border-zinc-800/80 bg-zinc-950/70 text-[11px] text-zinc-500 shadow-[0_12px_32px_rgba(0,0,0,0.16)]">
                                        <div className="flex items-start gap-2.5 px-3 py-2.5">
                                            <div className="flex h-7 w-7 items-center justify-center rounded-lg border border-white/5 bg-black/10">
                                                <Loader2 className="h-4 w-4 animate-spin text-blue-300" />
                                            </div>
                                            <div className="flex min-w-0 flex-1 flex-col items-start gap-1.5">
                                                <div className="flex w-full items-start gap-2">
                                                    <div className="min-w-0 flex flex-1 items-center gap-2">
                                                        <div className="shrink-0 text-[11px] font-semibold text-zinc-100">
                                                            {prettyName}
                                                        </div>
                                                        {displayPath && (
                                                            <button
                                                                type="button"
                                                                onClick={() => onOpenFile(toolActivity.filePath)}
                                                                className="min-w-0 flex-1 truncate rounded-md border border-zinc-800/90 bg-zinc-900/45 px-1.5 py-0.5 text-left text-[10px] text-zinc-300 transition-colors hover:border-zinc-700 hover:bg-zinc-800/55"
                                                                title={toolActivity.filePath}
                                                            >
                                                                {displayPath}
                                                            </button>
                                                        )}
                                                    </div>
                                                    <div className="ml-auto flex shrink-0 items-center gap-1.5 pl-2">
                                                        <span className="text-[9px] font-semibold uppercase tracking-[0.14em] text-blue-300">
                                                            Streaming
                                                        </span>
                                                        <button
                                                            type="button"
                                                            onClick={() => setIsToolActivityExpanded((prev) => !prev)}
                                                            className="rounded-md p-0.5 text-zinc-500 transition-colors hover:bg-zinc-800/80 hover:text-zinc-300"
                                                            title={isToolActivityExpanded ? 'Hide details' : 'Show details'}
                                                        >
                                                            {isToolActivityExpanded ? (
                                                                <ChevronDown className="h-3 w-3" />
                                                            ) : (
                                                                <ChevronRight className="h-3 w-3" />
                                                            )}
                                                        </button>
                                                    </div>
                                                </div>
                                                <div className="flex flex-wrap items-center gap-2 pl-0.5">
                                                    {toolActivity.chunkCount > 0 && (
                                                        <span className="text-[10px] font-medium text-blue-300">
                                                            {toolActivity.chunkCount} chunks streamed
                                                        </span>
                                                    )}
                                                </div>
                                            </div>
                                        </div>
                                        {isToolActivityExpanded && detailItems.length > 0 && (
                                            <div className="border-t border-zinc-800/60 bg-black/10 px-3 py-2.5">
                                                <div className="space-y-2">
                                                    {detailItems.map((item) => (
                                                        <div key={item.label} className="space-y-1">
                                                            <div className="text-[9px] font-semibold uppercase tracking-[0.16em] text-zinc-500">
                                                                {item.label}
                                                            </div>
                                                            <div className="wrap-break-word text-[11px] leading-5 text-zinc-300">
                                                                {item.value}
                                                            </div>
                                                        </div>
                                                    ))}
                                                </div>
                                            </div>
                                        )}
                                    </div>
                                </div>
                            );
                        })()}

                        <div className="h-4" />
                    </div>

                    {showScrollToBottom && activeTab === 'chat' && (
                        <div className="pointer-events-none sticky bottom-0 z-10 flex justify-center px-4 pb-4">
                            <button
                                onClick={scrollToBottom}
                                className="pointer-events-auto inline-flex items-center gap-2 rounded-full border border-[var(--border-subtle)] bg-[var(--bg-surface)]/95 px-4 py-2 text-xs font-medium text-[var(--fg-secondary)] shadow-[0_18px_48px_rgba(0,0,0,0.25)] backdrop-blur-md transition-colors hover:text-[var(--fg-primary)]"
                            >
                                <ArrowDown className="h-3.5 w-3.5" />
                                Jump to latest
                            </button>
                        </div>
                    )}
                </div>
            ) : (
                <HistoryTab
                    projectId={projectId}
                    onSelectConversation={handleSelectConversation}
                />
            )}

            {/* Global Accept/Reject All Changes - show immediately when changes exist */}
            <GlobalChangeActions
                changes={uncommittedChanges}
                onAcceptAll={onAcceptAllChanges}
                onRejectAll={onRejectAllChanges}
            />

            {/* Task Panel - persistent TODO above Command Center */}
            <TaskPanel
                todos={activeTodos}
                isCollapsed={taskPanelCollapsed}
                onToggleCollapse={() => setTaskPanelCollapsed(prev => !prev)}
            />

            <QueuePanel
                requests={queuedRequests}
                onEditRequest={handleEditQueuedRequest}
                onDeleteRequest={deleteQueuedRequest}
            />

            <CommandCenter
                onSend={sendMessage}
                onStop={stopGeneration}
                loading={loading}
                models={models}
                selectedModelId={selectedModelId}
                setSelectedModelId={setSelectedModelId}
                disabled={!hasApiKey}
                prefillRequest={composerPrefill}
                onPrefillConsumed={handleComposerPrefillConsumed}
            />

            {/* API Key Missing Overlay */}
            {!hasApiKey && (
                <div className="absolute inset-x-0 bottom-[140px] top-0 bg-black/60 backdrop-blur-sm flex flex-col items-center justify-center p-6 z-20 text-center animate-in fade-in duration-300">
                    <div className="bg-[var(--bg-surface)] border border-[var(--border-subtle)] p-6 rounded-xl shadow-2xl max-w-sm w-full">
                        <div className="w-12 h-12 bg-amber-500/10 rounded-full flex items-center justify-center mx-auto mb-4">
                            <Key className="w-6 h-6 text-amber-500" />
                        </div>
                        <h3 className="text-lg font-semibold text-[var(--fg-primary)] mb-2">Setup Required</h3>
                        <p className="text-sm text-[var(--fg-secondary)] mb-6">
                            To use the AI Assistant, you need to configure your Zaguán API Key.
                        </p>
                        <button
                            onClick={() => {
                                // Dispatch event to open settings
                                // Since we don't have direct access to setIsSettingsOpen, we can dispatch a custom event
                                // or rely on the user clicking the gear icon.
                                // But for better UX, let's try to emit an event Layout listens to?
                                // Layout listens for 'open-settings' maybe?
                                // For now, we'll suggest using the gear icon if we can't trigger it.
                                document.dispatchEvent(new CustomEvent('open-settings'));
                            }}
                            className="w-full py-2.5 px-4 bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg font-medium transition-colors flex items-center justify-center gap-2"
                        >
                            <Settings className="w-4 h-4" />
                            Open Settings
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
};

export const ChatPanel = React.memo(ChatPanelComponent);
