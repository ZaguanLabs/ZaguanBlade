import React, { useEffect, useRef, useCallback, useState, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { ArrowDown, Check, X, Settings, Key, Loader2 } from 'lucide-react';
import { useCommandExecution } from '../hooks/useCommandExecution';
import { useHistory } from '../hooks/useHistory';
import type { ChatMessage as ChatMessageType, ChatMode, ComposerMention, ImageAttachment, ModelInfo, QueuedRequest, ToolActivityState } from '../types/chat';

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
import { deriveChatRows, estimateChatRowHeight, findFirstUnvirtualizedChatRowIndex } from '../utils/chatTimeline';

const VIRTUALIZATION_OVERSCAN_PX = 720;

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
    sendMessage: (text: string, attachments?: ImageAttachment[], mentions?: ComposerMention[], mode?: ChatMode) => void;
    stopGeneration: () => void;
    models: ModelInfo[];
    selectedModelId: string;
    setSelectedModelId: (modelId: string) => void;
    chatMode: ChatMode;
    setChatMode: (mode: ChatMode) => void;
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
    onImplementPlan: (planText: string) => void;
}

function getPlanTextFromMessage(message: ChatMessageType): string | null {
    const content = message.content?.trim();
    if (content) {
        return content;
    }

    const blockText = (message.blocks || [])
        .filter((block): block is Extract<typeof block, { type: 'text' }> => block.type === 'text')
        .map((block) => block.content.trim())
        .filter(Boolean)
        .join('\n\n')
        .trim();
    if (blockText) {
        return blockText;
    }

    if (message.planSummary?.todos?.length) {
        return message.planSummary.todos
            .map((todo, index) => `${index + 1}. ${todo.content}`)
            .join('\n');
    }

    return null;
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
    chatMode,
    setChatMode,
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
    onImplementPlan,
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
    const showScrollToBottomRef = useRef(false);
    const [scrollMetrics, setScrollMetrics] = useState({
        scrollTop: 0,
        viewportHeight: 0,
        viewportWidth: 0,
    });

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
        const unlistenPromise = listen('remote-settings-changed', checkApiKey);
        return () => {
            unlistenPromise.then(unlisten => unlisten());
        };
    }, [checkApiKey]);

    const scrollContainerRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        const container = scrollContainerRef.current;
        if (!container) {
            return;
        }

        const updateMetrics = () => {
            setScrollMetrics((current) => {
                const next = {
                    scrollTop: container.scrollTop,
                    viewportHeight: container.clientHeight,
                    viewportWidth: container.clientWidth,
                };
                if (
                    current.scrollTop === next.scrollTop
                    && current.viewportHeight === next.viewportHeight
                    && current.viewportWidth === next.viewportWidth
                ) {
                    return current;
                }
                return next;
            });
        };

        updateMetrics();

        if (typeof ResizeObserver === 'undefined') {
            return;
        }

        const observer = new ResizeObserver(() => {
            updateMetrics();
        });
        observer.observe(container);
        return () => {
            observer.disconnect();
        };
    }, [activeTab]);

    const messageCount = messages.length;
    const firstMessageId = messages[0]?.id;
    const lastMessage = messages[messages.length - 1];
    const lastMessageId = lastMessage?.id;
    const lastMessageRole = lastMessage?.role;
    const lastMessageContent = lastMessage?.content ?? '';
    const lastMessageReasoning = lastMessage?.reasoning ?? '';
    const lastMessageBlockCount = lastMessage?.blocks?.length ?? 0;
    const shouldShowPendingResponseIndicator = loading && lastMessage?.role !== 'Assistant';
    const chatRows = useMemo(() => deriveChatRows(messages, loading, pendingActions), [loading, messages, pendingActions]);
    const firstUnvirtualizedRowIndex = useMemo(
        () => findFirstUnvirtualizedChatRowIndex(chatRows, loading),
        [chatRows, loading]
    );
    const virtualizedRows = useMemo(
        () => chatRows.slice(0, firstUnvirtualizedRowIndex),
        [chatRows, firstUnvirtualizedRowIndex]
    );
    const tailRows = useMemo(
        () => chatRows.slice(firstUnvirtualizedRowIndex),
        [chatRows, firstUnvirtualizedRowIndex]
    );
    const virtualizedRowHeights = useMemo(
        () => virtualizedRows.map((row) => estimateChatRowHeight(row, { viewportWidthPx: scrollMetrics.viewportWidth })),
        [scrollMetrics.viewportWidth, virtualizedRows]
    );
    const virtualizedRowOffsets = useMemo(() => {
        const offsets: number[] = [];
        let runningTotal = 0;
        for (const height of virtualizedRowHeights) {
            offsets.push(runningTotal);
            runningTotal += height;
        }
        return offsets;
    }, [virtualizedRowHeights]);
    const totalVirtualizedHeight = useMemo(
        () => virtualizedRowHeights.reduce((sum, height) => sum + height, 0),
        [virtualizedRowHeights]
    );
    const visibleVirtualRange = useMemo(() => {
        const rowCount = virtualizedRows.length;
        if (rowCount === 0) {
            return { startIndex: 0, endIndex: 0, topSpacerHeight: 0, bottomSpacerHeight: 0 };
        }

        const viewportStart = Math.max(0, scrollMetrics.scrollTop - VIRTUALIZATION_OVERSCAN_PX);
        const viewportEnd = scrollMetrics.scrollTop + scrollMetrics.viewportHeight + VIRTUALIZATION_OVERSCAN_PX;

        let startIndex = 0;
        while (
            startIndex < rowCount
            && virtualizedRowOffsets[startIndex] + virtualizedRowHeights[startIndex] < viewportStart
        ) {
            startIndex += 1;
        }

        let endIndex = startIndex;
        while (endIndex < rowCount && virtualizedRowOffsets[endIndex] < viewportEnd) {
            endIndex += 1;
        }

        const topSpacerHeight = virtualizedRowOffsets[startIndex] ?? totalVirtualizedHeight;
        const renderedHeight = virtualizedRowHeights
            .slice(startIndex, endIndex)
            .reduce((sum, height) => sum + height, 0);
        const bottomSpacerHeight = Math.max(0, totalVirtualizedHeight - topSpacerHeight - renderedHeight);

        return { startIndex, endIndex, topSpacerHeight, bottomSpacerHeight };
    }, [scrollMetrics.scrollTop, scrollMetrics.viewportHeight, totalVirtualizedHeight, virtualizedRowHeights, virtualizedRowOffsets, virtualizedRows.length]);
    const visibleVirtualRows = useMemo(
        () => virtualizedRows.slice(visibleVirtualRange.startIndex, visibleVirtualRange.endIndex),
        [virtualizedRows, visibleVirtualRange.endIndex, visibleVirtualRange.startIndex]
    );
    const streamingSignature = useMemo(() => {
        if (!lastMessage) return '';
        return [
            lastMessageId ?? messageCount,
            lastMessageContent,
            lastMessageReasoning,
            lastMessageBlockCount,
        ].join('|');
    }, [lastMessageBlockCount, lastMessageContent, lastMessageId, lastMessageReasoning, messageCount]);

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
        setScrollMetrics((current) => {
            const next = {
                scrollTop: target.scrollTop,
                viewportHeight: target.clientHeight,
                viewportWidth: target.clientWidth,
            };
            if (
                current.scrollTop === next.scrollTop
                && current.viewportHeight === next.viewportHeight
                && current.viewportWidth === next.viewportWidth
            ) {
                return current;
            }
            return next;
        });
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

    const latestPlanText = useMemo(() => {
        for (let idx = messages.length - 1; idx >= 0; idx -= 1) {
            const message = messages[idx];
            if (message.role !== 'Assistant') {
                continue;
            }
            const planText = getPlanTextFromMessage(message);
            if (planText) {
                return planText;
            }
        }
        return null;
    }, [messages]);

    const handleImplementPlan = useCallback(() => {
        if (!latestPlanText) return;
        onImplementPlan(latestPlanText);
    }, [latestPlanText, onImplementPlan]);

    const showProgressIndicator = loading
        && researchProgress?.isActive
        && researchProgress.stage.toLowerCase() !== 'considering_next_steps';

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
                    <div className="mx-auto flex w-full max-w-none flex-col gap-0.5 px-0.5 py-4 md:px-1">
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

                        {visibleVirtualRange.topSpacerHeight > 0 && (
                            <div style={{ height: `${visibleVirtualRange.topSpacerHeight}px` }} />
                        )}

                        {visibleVirtualRows.map((row) => (
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

                        {visibleVirtualRange.bottomSpacerHeight > 0 && (
                            <div style={{ height: `${visibleVirtualRange.bottomSpacerHeight}px` }} />
                        )}

                        {tailRows.map((row) => (
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
                        {showProgressIndicator && (
                            <div className="px-4">
                                <ProgressIndicator progress={researchProgress} />
                            </div>
                        )}

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

            {chatMode === 'planning' && latestPlanText && (
                <div className="px-3 pb-1 pt-2">
                    <div className="flex justify-end">
                        <button
                            type="button"
                            onClick={handleImplementPlan}
                            disabled={loading || !hasApiKey}
                            className="inline-flex items-center rounded-md border border-emerald-500/30 bg-emerald-500/12 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.14em] text-emerald-200 transition-colors hover:border-emerald-400/50 hover:bg-emerald-500/20 disabled:cursor-not-allowed disabled:opacity-40"
                        >
                            Implement
                        </button>
                    </div>
                </div>
            )}

            <CommandCenter
                onSend={sendMessage}
                onStop={stopGeneration}
                loading={loading}
                models={models}
                selectedModelId={selectedModelId}
                setSelectedModelId={setSelectedModelId}
                chatMode={chatMode}
                setChatMode={setChatMode}
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
