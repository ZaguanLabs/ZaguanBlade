import React, { useEffect, useRef, useCallback, useState, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowDown, Check, X, Loader2 } from 'lucide-react';
import { useCommandExecution } from '../hooks/useCommandExecution';
import { useHistory } from '../hooks/useHistory';
import type { ChatMessage as ChatMessageType, ChatMode, ComposerMention, HookApprovalRequest, ImageAttachment, ModelInfo, QueuedRequest, ToolActivityState } from '../types/chat';

import type { StructuredAction, TodoItem } from '../types/events';
import { ChatMessage } from './ChatMessage';
import { ChatTabBar } from './ChatTabBar';
import { CommandCenter } from './CommandCenter';
import { HistoryTab } from './HistoryTab';
import { ProgressIndicator } from './ProgressIndicator';
import { GlobalChangeActions } from './editor/GlobalChangeActions';
import { TaskPanel } from './TaskPanel';
import { QueuePanel } from './QueuePanel';
import { AgentRunStatusBar } from './AgentRunStatusBar';
import type { UncommittedChange } from '../types/uncommitted';
import { deriveChatRows, estimateChatRowHeight, findFirstUnvirtualizedChatRowIndex } from '../utils/chatTimeline';
import { recordDebugPerf } from '../utils/debugPerf';

const VIRTUALIZATION_OVERSCAN_PX = 720;
const SCROLL_BOTTOM_THRESHOLD_PX = 80;
const VIRTUALIZATION_SCROLL_THROTTLE_MS = 150;

interface VisibleVirtualRange {
    startIndex: number;
    endIndex: number;
    topSpacerHeight: number;
    bottomSpacerHeight: number;
}

function sameVisibleVirtualRange(a: VisibleVirtualRange, b: VisibleVirtualRange): boolean {
    return a.startIndex === b.startIndex
        && a.endIndex === b.endIndex
        && a.topSpacerHeight === b.topSpacerHeight
        && a.bottomSpacerHeight === b.bottomSpacerHeight;
}

function computeVisibleVirtualRange(
    scrollTop: number,
    viewportHeight: number,
    virtualizedRowOffsets: number[],
    virtualizedRowHeights: number[],
    totalVirtualizedHeight: number,
): VisibleVirtualRange {
    const rowCount = virtualizedRowHeights.length;
    if (rowCount === 0) {
        return { startIndex: 0, endIndex: 0, topSpacerHeight: 0, bottomSpacerHeight: 0 };
    }

    const viewportStart = Math.max(0, scrollTop - VIRTUALIZATION_OVERSCAN_PX);
    const viewportEnd = scrollTop + viewportHeight + VIRTUALIZATION_OVERSCAN_PX;

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
    let renderedHeight = 0;
    for (let index = startIndex; index < endIndex; index += 1) {
        renderedHeight += virtualizedRowHeights[index] ?? 0;
    }
    const bottomSpacerHeight = Math.max(0, totalVirtualizedHeight - topSpacerHeight - renderedHeight);

    return { startIndex, endIndex, topSpacerHeight, bottomSpacerHeight };
}

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
    pendingApprovalRequest: HookApprovalRequest | null;
    waitingForApproval: boolean;
    approveToolDecision: (decision: string) => void;
    respondToApprovalRequest: (approved: boolean) => void;
    approveSingleCommand: (callId: string) => void;
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

const PendingResponseIndicator: React.FC = () => {
    const { t } = useTranslation();
    const [wordIndex, setWordIndex] = useState(0);
    const words = useMemo(() => [
        t('chat.pendingResponse.working'),
        t('chat.pendingResponse.thinking'),
        t('chat.pendingResponse.waiting'),
        t('chat.pendingResponse.ruminating'),
    ], [t]);

    // Slow word rotation (4s instead of 1.8s) to reduce DOM mutations
    // that keep WebKitGTK's frame clock cycling.
    useEffect(() => {
        const intervalId = window.setInterval(() => {
            setWordIndex((prev) => (prev + 1) % words.length);
        }, 4000);

        return () => window.clearInterval(intervalId);
    }, [words.length]);

    return (
        <div className="px-4 py-3">
            <div className="inline-flex items-center gap-3 rounded-2xl border border-emerald-500/15 bg-[linear-gradient(180deg,rgba(16,185,129,0.08),rgba(24,24,27,0.7))] px-4 py-3 text-[11px] font-medium text-(--fg-secondary) shadow-[0_16px_40px_rgba(0,0,0,0.18)]">
                <div className="flex h-8 w-8 items-center justify-center rounded-2xl border border-emerald-500/20 bg-emerald-500/10">
                    <Loader2 className="h-4 w-4 text-emerald-300" />
                </div>
                <div className="flex flex-col items-start">
                    <span className="text-[10px] font-semibold uppercase tracking-[0.18em] text-emerald-300/80">
                        {t('chat.assistantResponding')}
                    </span>
                    <span key={words[wordIndex]} className="text-sm font-semibold text-(--fg-primary)">
                        {words[wordIndex]}
                    </span>
                </div>
                <div className="ml-1 flex items-center gap-1">
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-400/80" />
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-400/60" />
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-400/40" />
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
    pendingApprovalRequest,
    waitingForApproval,
    approveToolDecision,
    respondToApprovalRequest,
    approveSingleCommand,
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
    recordDebugPerf('ChatPanel.render');
    const { t } = useTranslation();
    const { stopCommandExecution } = useCommandExecution();
    const [taskPanelCollapsed, setTaskPanelCollapsed] = useState(false);
    const { loadConversation } = useHistory();
    const isUserAtBottomRef = useRef(true);
    const prevMessageCountRef = useRef(0);
    const [activeTab, setActiveTab] = useState<'chat' | 'history'>('chat');
    const [composerPrefill, setComposerPrefill] = useState<QueuedRequest | null>(null);
    const [showScrollToBottom, setShowScrollToBottom] = useState(false);
    const showScrollToBottomRef = useRef(false);
    const canUseAi = models.length > 0;
    const rowElementsRef = useRef(new Map<string, HTMLDivElement>());
    const activityElementsRef = useRef(new Map<string, HTMLDivElement>());
    const jumpHighlightTimersRef = useRef(new WeakMap<HTMLElement, number>());
    const [pendingRowJumpKey, setPendingRowJumpKey] = useState<string | null>(null);
    const [pendingActivityJumpKey, setPendingActivityJumpKey] = useState<string | null>(null);
    const taskPanelRef = useRef<HTMLDivElement>(null);
    const queuePanelRef = useRef<HTMLDivElement>(null);
    const [viewportMetrics, setViewportMetrics] = useState({
        viewportHeight: 0,
        viewportWidth: 0,
    });
    const [visibleVirtualRange, setVisibleVirtualRange] = useState<VisibleVirtualRange>({
        startIndex: 0,
        endIndex: 0,
        topSpacerHeight: 0,
        bottomSpacerHeight: 0,
    });

    const scrollContainerRef = useRef<HTMLDivElement>(null);
    const bottomSentinelRef = useRef<HTMLDivElement>(null);
    const viewportMetricsFrameRef = useRef<number | null>(null);
    const pendingViewportMetricsRef = useRef<{
        viewportHeight: number;
        viewportWidth: number;
    } | null>(null);
    const visibleRangeFrameRef = useRef<number | null>(null);
    const scrollTopRef = useRef(0);
    const viewportHeightRef = useRef(0);
    const virtualizedRowOffsetsRef = useRef<number[]>([]);
    const virtualizedRowHeightsRef = useRef<number[]>([]);
    const totalVirtualizedHeightRef = useRef(0);
    const virtualizedRowHeightCacheRef = useRef(new WeakMap<object, Map<string, number>>());
    const virtualizationScrollTimerRef = useRef<number | null>(null);
    const lastVirtualizationScrollUpdateRef = useRef(0);

    const scheduleVisibleVirtualRangeUpdate = useCallback((scrollTop?: number, immediate?: boolean) => {
        if (typeof scrollTop === 'number') {
            scrollTopRef.current = scrollTop;
        }

        if (visibleRangeFrameRef.current !== null) {
            return;
        }

        const runUpdate = () => {
            visibleRangeFrameRef.current = null;
            recordDebugPerf('ChatPanel.visibleRangeUpdate');
            const nextRange = computeVisibleVirtualRange(
                scrollTopRef.current,
                viewportHeightRef.current,
                virtualizedRowOffsetsRef.current,
                virtualizedRowHeightsRef.current,
                totalVirtualizedHeightRef.current,
            );
            setVisibleVirtualRange((current) => sameVisibleVirtualRange(current, nextRange) ? current : nextRange);
        };

        if (immediate) {
            runUpdate();
        } else {
            visibleRangeFrameRef.current = requestAnimationFrame(runUpdate);
        }
    }, []);

    const scheduleViewportMetricsUpdate = useCallback((element: HTMLDivElement) => {
        pendingViewportMetricsRef.current = {
            viewportHeight: element.clientHeight,
            viewportWidth: element.clientWidth,
        };

        if (viewportMetricsFrameRef.current !== null) {
            return;
        }

        viewportMetricsFrameRef.current = requestAnimationFrame(() => {
            viewportMetricsFrameRef.current = null;
            recordDebugPerf('ChatPanel.viewportMetricsFrame');
            const next = pendingViewportMetricsRef.current;
            if (!next) {
                return;
            }

            viewportHeightRef.current = next.viewportHeight;

            setViewportMetrics((current) => {
                if (
                    current.viewportHeight === next.viewportHeight
                    && current.viewportWidth === next.viewportWidth
                ) {
                    return current;
                }
                return next;
            });

            scheduleVisibleVirtualRangeUpdate(scrollTopRef.current);
        });
    }, [scheduleVisibleVirtualRangeUpdate]);

    useEffect(() => {
        const container = scrollContainerRef.current;
        if (!container) {
            return;
        }

        scrollTopRef.current = container.scrollTop;

        const updateMetrics = () => {
            scheduleViewportMetricsUpdate(container);
        };

        updateMetrics();

        if (typeof ResizeObserver === 'undefined') {
            return () => {
                if (viewportMetricsFrameRef.current !== null) {
                    cancelAnimationFrame(viewportMetricsFrameRef.current);
                    viewportMetricsFrameRef.current = null;
                }
                if (visibleRangeFrameRef.current !== null) {
                    cancelAnimationFrame(visibleRangeFrameRef.current);
                    visibleRangeFrameRef.current = null;
                }
            };
        }

        const observer = new ResizeObserver(() => {
            recordDebugPerf('ChatPanel.resizeObserver');
            updateMetrics();
        });
        observer.observe(container);
        return () => {
            if (viewportMetricsFrameRef.current !== null) {
                cancelAnimationFrame(viewportMetricsFrameRef.current);
                viewportMetricsFrameRef.current = null;
            }
            if (visibleRangeFrameRef.current !== null) {
                cancelAnimationFrame(visibleRangeFrameRef.current);
                visibleRangeFrameRef.current = null;
            }
            observer.disconnect();
        };
    }, [scheduleViewportMetricsUpdate]);

    const messageCount = messages.length;
    const firstMessageId = messages[0]?.id;
    const lastMessage = messages[messages.length - 1];
    const lastMessageId = lastMessage?.id;
    const lastMessageRole = lastMessage?.role;
    const lastMessageContent = lastMessage?.content ?? '';
    const lastMessageReasoning = lastMessage?.reasoning ?? '';
    const lastMessageBlockCount = lastMessage?.blocks?.length ?? 0;
    const shouldShowPendingResponseIndicator = loading && !waitingForApproval && lastMessage?.role !== 'Assistant';
    const chatRows = useMemo(
        () => deriveChatRows(messages, loading, pendingActions, pendingApprovalRequest),
        [loading, messages, pendingActions, pendingApprovalRequest],
    );
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
        () => virtualizedRows.map((row) => {
            const cacheKey = [
                viewportMetrics.viewportWidth,
                row.isContinued ? 1 : 0,
                row.isActive ? 1 : 0,
                row.pendingActions?.length ?? 0,
            ].join('|');
            const cacheByLayout = virtualizedRowHeightCacheRef.current.get(row.message);
            const cachedHeight = cacheByLayout?.get(cacheKey);
            if (typeof cachedHeight === 'number') {
                return cachedHeight;
            }

            const estimatedHeight = estimateChatRowHeight(row, { viewportWidthPx: viewportMetrics.viewportWidth });
            const nextCacheByLayout = cacheByLayout ?? new Map<string, number>();
            nextCacheByLayout.set(cacheKey, estimatedHeight);
            if (!cacheByLayout) {
                virtualizedRowHeightCacheRef.current.set(row.message, nextCacheByLayout);
            }
            return estimatedHeight;
        }),
        [viewportMetrics.viewportWidth, virtualizedRows]
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
    const visibleVirtualRows = useMemo(
        () => virtualizedRows.slice(visibleVirtualRange.startIndex, visibleVirtualRange.endIndex),
        [virtualizedRows, visibleVirtualRange.endIndex, visibleVirtualRange.startIndex]
    );
    useEffect(() => {
        virtualizedRowOffsetsRef.current = virtualizedRowOffsets;
        virtualizedRowHeightsRef.current = virtualizedRowHeights;
        totalVirtualizedHeightRef.current = totalVirtualizedHeight;
        viewportHeightRef.current = viewportMetrics.viewportHeight;
        scheduleVisibleVirtualRangeUpdate(scrollTopRef.current);
    }, [scheduleVisibleVirtualRangeUpdate, totalVirtualizedHeight, viewportMetrics.viewportHeight, virtualizedRowHeights, virtualizedRowOffsets]);
    const rowIndexByKey = useMemo(() => {
        const indexMap = new Map<string, number>();
        chatRows.forEach((row, index) => {
            indexMap.set(row.key, index);
        });
        return indexMap;
    }, [chatRows]);
    const approvalTargetKey = useMemo(
        () => pendingApprovalRequest?.toolCallId
            ? `approval:${pendingApprovalRequest.toolCallId}`
            : pendingActions?.[0]?.id
                ? `approval:${pendingActions[0].id}`
                : null,
        [pendingActions, pendingApprovalRequest],
    );
    const approvalRowKey = useMemo(
        () => chatRows.find((row) => (row.pendingActions?.length ?? 0) > 0 || !!row.pendingApprovalRequest)?.key ?? null,
        [chatRows],
    );
    const activeStepTargetKey = useMemo(
        () => toolActivity?.toolCallId ? `tool:${toolActivity.toolCallId}` : null,
        [toolActivity?.toolCallId],
    );
    const activeStepRowKey = useMemo(() => {
        if (toolActivity?.toolCallId) {
            const matchingRow = [...chatRows].reverse().find((row) =>
                row.message.tool_calls?.some((toolCall) => toolCall.id === toolActivity.toolCallId),
            );
            if (matchingRow) {
                return matchingRow.key;
            }
        }

        const activeRow = chatRows.find((row) => row.isActive);
        if (activeRow) {
            return activeRow.key;
        }

        const lastAssistantRow = [...chatRows].reverse().find((row) => row.message.role === 'Assistant');
        return lastAssistantRow?.key ?? null;
    }, [chatRows, toolActivity?.toolCallId]);
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
        scrollTopRef.current = container.scrollTop;
        scheduleVisibleVirtualRangeUpdate(container.scrollTop, true);
        // NOTE: Do NOT set isUserAtBottomRef here — only handleScroll should
        // update that flag, otherwise programmatic scrolls override user intent.
        if (showScrollToBottomRef.current) {
            showScrollToBottomRef.current = false;
            setShowScrollToBottom(false);
        }
    }, [scheduleVisibleVirtualRangeUpdate]);

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

    // IntersectionObserver-based sticky bottom for streaming.
    // Instead of forcing scrollToBottom on every streaming token (which fights
    // the user's wheel scroll), we observe a sentinel div at the bottom. When
    // the user is near the bottom and new content arrives, the browser's native
    // overflow-anchor keeps the scroll position anchored. We only call
    // scrollToBottom when the sentinel is visible (meaning user is at bottom)
    // and new content has been appended that might push the sentinel out of view.
    const streamingScrollFrameRef = useRef<number | null>(null);
    const isSentinelVisibleRef = useRef(true);

    useEffect(() => {
        const sentinel = bottomSentinelRef.current;
        if (!sentinel) return;

        const observer = new IntersectionObserver(
            ([entry]) => {
                isSentinelVisibleRef.current = entry.isIntersecting;
            },
            {
                root: scrollContainerRef.current,
                threshold: 0,
                rootMargin: `${SCROLL_BOTTOM_THRESHOLD_PX}px 0px 0px 0px`,
            },
        );

        observer.observe(sentinel);
        return () => observer.disconnect();
    }, []);

    // During streaming, gently nudge scroll to bottom only if the sentinel
    // is visible (user is at/near bottom). This replaces the old
    // streamingSignature-driven scrollToBottom which fought the user's wheel.
    useEffect(() => {
        if (!loading) return;
        if (!isSentinelVisibleRef.current) return;
        if (!isUserAtBottomRef.current) return;

        if (streamingScrollFrameRef.current !== null) {
            return;
        }

        streamingScrollFrameRef.current = requestAnimationFrame(() => {
            streamingScrollFrameRef.current = null;
            recordDebugPerf('ChatPanel.streamingScrollFrame');
            const container = scrollContainerRef.current;
            if (!container) return;
            // Only nudge if not already at bottom — avoids fighting user scroll
            const distanceFromBottom = container.scrollHeight - container.scrollTop - container.clientHeight;
            if (distanceFromBottom > 2) {
                container.scrollTop = container.scrollHeight;
                scrollTopRef.current = container.scrollTop;
            }
        });
        return () => {
            if (streamingScrollFrameRef.current !== null) {
                cancelAnimationFrame(streamingScrollFrameRef.current);
                streamingScrollFrameRef.current = null;
            }
        };
    }, [loading, streamingSignature]);

    // Prevent default context menu on empty areas
    const handleContextMenu = useCallback((e: React.MouseEvent) => {
        // Always prevent default to avoid native Tauri menu
        e.preventDefault();
    }, []);

    const handleScroll = useCallback((e: React.UIEvent<HTMLDivElement>) => {
        recordDebugPerf('ChatPanel.scroll');
        const target = e.target as HTMLDivElement;
        scrollTopRef.current = target.scrollTop;

        // Throttle virtualization updates during manual scrolling to prevent
        // React state churn on every wheel event.
        const now = Date.now();
        if (now - lastVirtualizationScrollUpdateRef.current >= VIRTUALIZATION_SCROLL_THROTTLE_MS) {
            lastVirtualizationScrollUpdateRef.current = now;
            scheduleVisibleVirtualRangeUpdate(target.scrollTop);
        } else {
            // Still update the ref immediately for accuracy, but defer the
            // expensive state computation.
            if (virtualizationScrollTimerRef.current === null) {
                virtualizationScrollTimerRef.current = window.setTimeout(() => {
                    virtualizationScrollTimerRef.current = null;
                    lastVirtualizationScrollUpdateRef.current = Date.now();
                    scheduleVisibleVirtualRangeUpdate(scrollTopRef.current);
                }, VIRTUALIZATION_SCROLL_THROTTLE_MS);
            }
        }

        const isBottom = Math.abs(target.scrollHeight - target.scrollTop - target.clientHeight) < SCROLL_BOTTOM_THRESHOLD_PX;
        isUserAtBottomRef.current = isBottom;
        const nextShowScrollToBottom = !isBottom && messageCount > 0;
        if (showScrollToBottomRef.current !== nextShowScrollToBottom) {
            showScrollToBottomRef.current = nextShowScrollToBottom;
            setShowScrollToBottom(nextShowScrollToBottom);
        }
    }, [messageCount, scheduleVisibleVirtualRangeUpdate]);

    const registerRowElement = useCallback((rowKey: string, element: HTMLDivElement | null) => {
        if (element) {
            rowElementsRef.current.set(rowKey, element);
            return;
        }
        rowElementsRef.current.delete(rowKey);
    }, []);

    const registerActivityTarget = useCallback((targetKey: string, element: HTMLDivElement | null) => {
        if (element) {
            activityElementsRef.current.set(targetKey, element);
            return;
        }
        activityElementsRef.current.delete(targetKey);
    }, []);

    const flashJumpTarget = useCallback((element: HTMLElement | null) => {
        if (!element) {
            return;
        }

        const activeTimer = jumpHighlightTimersRef.current.get(element);
        if (activeTimer) {
            window.clearTimeout(activeTimer);
        }

        element.style.transition = 'box-shadow 180ms ease, background-color 180ms ease';
        element.style.boxShadow = '0 0 0 1px color-mix(in srgb, var(--accent-primary) 42%, transparent)';
        element.style.backgroundColor = 'color-mix(in srgb, var(--accent-primary) 8%, transparent)';

        const timerId = window.setTimeout(() => {
            element.style.boxShadow = '';
            element.style.backgroundColor = '';
            element.style.transition = '';
            jumpHighlightTimersRef.current.delete(element);
        }, 1400);

        jumpHighlightTimersRef.current.set(element, timerId);
    }, []);

    const scrollToActivityTarget = useCallback((targetKey: string | null) => {
        if (!targetKey) {
            return false;
        }
        const targetElement = activityElementsRef.current.get(targetKey);
        if (!targetElement) {
            return false;
        }
        targetElement.scrollIntoView({ block: 'center', behavior: 'smooth' });
        flashJumpTarget(targetElement);
        return true;
    }, [flashJumpTarget]);

    const scrollToRow = useCallback((rowKey: string, activityTargetKey?: string | null) => {
        const container = scrollContainerRef.current;
        if (!container) return;

        const rowIndex = rowIndexByKey.get(rowKey);
        if (rowIndex === undefined) return;

        if (rowIndex < firstUnvirtualizedRowIndex) {
            const targetTop = Math.max(0, (virtualizedRowOffsets[rowIndex] ?? 0) - 120);
            container.scrollTo({ top: targetTop, behavior: 'smooth' });
            requestAnimationFrame(() => {
                if (scrollToActivityTarget(activityTargetKey ?? null)) {
                    return;
                }
                const rowElement = rowElementsRef.current.get(rowKey);
                rowElement?.scrollIntoView({ block: 'center', behavior: 'smooth' });
                flashJumpTarget(rowElement ?? null);
            });
            return;
        }

        if (scrollToActivityTarget(activityTargetKey ?? null)) {
            return;
        }
        const rowElement = rowElementsRef.current.get(rowKey);
        rowElement?.scrollIntoView({ block: 'center', behavior: 'smooth' });
        flashJumpTarget(rowElement ?? null);
    }, [firstUnvirtualizedRowIndex, flashJumpTarget, rowIndexByKey, scrollToActivityTarget, virtualizedRowOffsets]);

    const jumpToRow = useCallback((rowKey: string | null, activityTargetKey?: string | null) => {
        if (!rowKey) return;
        if (activeTab !== 'chat') {
            setPendingRowJumpKey(rowKey);
            setPendingActivityJumpKey(activityTargetKey ?? null);
            setActiveTab('chat');
            return;
        }
        scrollToRow(rowKey, activityTargetKey);
    }, [activeTab, scrollToRow]);

    useEffect(() => {
        if (activeTab !== 'chat' || !pendingRowJumpKey) {
            return;
        }
        const rafId = requestAnimationFrame(() => {
            scrollToRow(pendingRowJumpKey, pendingActivityJumpKey);
            setPendingRowJumpKey(null);
            setPendingActivityJumpKey(null);
        });
        return () => cancelAnimationFrame(rafId);
    }, [activeTab, pendingActivityJumpKey, pendingRowJumpKey, scrollToRow]);

    const jumpToTaskPanel = useCallback(() => {
        taskPanelRef.current?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    }, []);

    const jumpToQueue = useCallback(() => {
        queuePanelRef.current?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    }, []);

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

    const handleApproveApprovalRequest = useCallback(() => {
        respondToApprovalRequest(true);
    }, [respondToApprovalRequest]);

    const handleDenyApprovalRequest = useCallback(() => {
        respondToApprovalRequest(false);
    }, [respondToApprovalRequest]);

    const handleApproveSingleCommand = useCallback((callId: string) => {
        approveSingleCommand(callId);
    }, [approveSingleCommand]);

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
                    className="relative flex-1 overflow-y-auto overflow-y-scroll scrollbar-thin scrollbar-thumb-zinc-800 scrollbar-track-transparent"
                    onScroll={handleScroll}
                >
                    <div className="mx-auto flex w-full max-w-none flex-col gap-0.5 px-0.5 py-4 md:px-1">
                        {messages.length === 0 && (
                            <div className="mx-4 mt-10 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]/70 px-6 py-8 text-center shadow-[0_24px_80px_rgba(0,0,0,0.25)]">
                                <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-2xl border border-emerald-500/20 bg-emerald-500/10 text-2xl shadow-[0_0_40px_rgba(16,185,129,0.15)]">
                                    🗡️
                                </div>
                                <h2 className="text-sm font-semibold uppercase tracking-[0.24em] text-[var(--fg-secondary)]">{t('app.name')}</h2>
                                <p className="mx-auto mt-3 max-w-md text-sm leading-6 text-[var(--fg-tertiary)]">
                                    {t('chat.emptyState.intro')}
                                </p>
                                <p className="mx-auto mt-3 max-w-md text-sm leading-6 text-[var(--fg-secondary)]">
                                    {t('chat.emptyState.tip')}
                                </p>
                                <div className="mt-5 flex flex-wrap items-center justify-center gap-2 text-[11px] text-[var(--fg-tertiary)]">
                                    <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--bg-app)] px-3 py-1">{t('chat.emptyState.understandFile')}</span>
                                    <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--bg-app)] px-3 py-1">{t('chat.emptyState.planNextChange')}</span>
                                    <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--bg-app)] px-3 py-1">{t('chat.emptyState.reviewCommandOutput')}</span>
                                    <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--bg-app)] px-3 py-1">{t('chat.emptyState.attachScreenshots')}</span>
                                </div>
                                <p className="mt-5 text-xs font-mono uppercase tracking-[0.2em] text-[var(--fg-tertiary)]/70">
                                    {t('chat.emptyState.startPrompt')}
                                </p>
                            </div>
                        )}

                        {visibleVirtualRange.topSpacerHeight > 0 && (
                            <div style={{ height: `${visibleVirtualRange.topSpacerHeight}px` }} />
                        )}

                        {visibleVirtualRows.map((row) => (
                            <div key={row.key} ref={(element) => registerRowElement(row.key, element)} data-chat-row-key={row.key}>
                                <ChatMessage
                                    message={row.message}
                                    pendingActions={row.pendingActions}
                                    pendingApprovalRequest={row.pendingApprovalRequest}
                                    onApproveCommand={row.pendingActions ? handleApproveCommand : undefined}
                                    onSkipCommand={row.pendingActions ? handleSkipCommand : undefined}
                                    onApproveApprovalRequest={row.pendingApprovalRequest ? handleApproveApprovalRequest : undefined}
                                    onDenyApprovalRequest={row.pendingApprovalRequest ? handleDenyApprovalRequest : undefined}
                                    onApproveSingleCommand={row.pendingActions ? handleApproveSingleCommand : undefined}
                                    onSkipSingleCommand={row.pendingActions ? handleSkipSingleCommand : undefined}
                                    isContinued={row.isContinued}
                                    isActive={row.isActive}
                                    onUndoTool={onUndoTool}
                                    onStopCommand={handleStopCommand}
                                    onOpenFile={onOpenFile}
                                    registerActivityTarget={registerActivityTarget}
                                />
                            </div>
                        ))}

                        {visibleVirtualRange.bottomSpacerHeight > 0 && (
                            <div style={{ height: `${visibleVirtualRange.bottomSpacerHeight}px` }} />
                        )}

                        {tailRows.map((row) => (
                            <div key={row.key} ref={(element) => registerRowElement(row.key, element)} data-chat-row-key={row.key}>
                                <ChatMessage
                                    message={row.message}
                                    pendingActions={row.pendingActions}
                                    pendingApprovalRequest={row.pendingApprovalRequest}
                                    onApproveCommand={row.pendingActions ? handleApproveCommand : undefined}
                                    onSkipCommand={row.pendingActions ? handleSkipCommand : undefined}
                                    onApproveApprovalRequest={row.pendingApprovalRequest ? handleApproveApprovalRequest : undefined}
                                    onDenyApprovalRequest={row.pendingApprovalRequest ? handleDenyApprovalRequest : undefined}
                                    onApproveSingleCommand={row.pendingActions ? handleApproveSingleCommand : undefined}
                                    onSkipSingleCommand={row.pendingActions ? handleSkipSingleCommand : undefined}
                                    isContinued={row.isContinued}
                                    isActive={row.isActive}
                                    onUndoTool={onUndoTool}
                                    onStopCommand={handleStopCommand}
                                    onOpenFile={onOpenFile}
                                    registerActivityTarget={registerActivityTarget}
                                />
                            </div>
                        ))}

                        {shouldShowPendingResponseIndicator && <PendingResponseIndicator />}

                        {/* Research progress indicator */}
                        {showProgressIndicator && (
                            <div className="px-4">
                                <ProgressIndicator progress={researchProgress} />
                            </div>
                        )}

                        <div className="h-4" />
                        {/* Bottom sentinel for IntersectionObserver-based sticky scroll */}
                        <div ref={bottomSentinelRef} className="h-0" />
                    </div>

                    {showScrollToBottom && activeTab === 'chat' && (
                        <div className="pointer-events-none sticky bottom-0 z-10 flex justify-center px-4 pb-4">
                            <button
                                onClick={scrollToBottom}
                                className="pointer-events-auto inline-flex items-center gap-2 rounded-full border border-[var(--border-subtle)] bg-[var(--bg-surface)]/95 px-4 py-2 text-xs font-medium text-[var(--fg-secondary)] shadow-[0_18px_48px_rgba(0,0,0,0.25)] transition-colors hover:text-[var(--fg-primary)]"
                            >
                                <ArrowDown className="h-3.5 w-3.5" />
                                {t('chat.jumpToLatest')}
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
            <div ref={taskPanelRef}>
                <TaskPanel
                    todos={activeTodos}
                    isCollapsed={taskPanelCollapsed}
                    onToggleCollapse={() => setTaskPanelCollapsed(prev => !prev)}
                />
            </div>

            <div ref={queuePanelRef}>
                <QueuePanel
                    requests={queuedRequests}
                    onEditRequest={handleEditQueuedRequest}
                    onDeleteRequest={deleteQueuedRequest}
                />
            </div>

            <AgentRunStatusBar
                loading={loading}
                pendingActions={pendingActions}
                pendingApprovalRequest={pendingApprovalRequest}
                waitingForApproval={waitingForApproval}
                toolActivity={toolActivity}
                activeTodos={activeTodos}
                queuedRequests={queuedRequests}
                onStop={stopGeneration}
                onJumpToApproval={() => jumpToRow(approvalRowKey, approvalTargetKey)}
                onJumpToActiveStep={() => jumpToRow(activeStepRowKey, activeStepTargetKey)}
                onJumpToTaskPanel={jumpToTaskPanel}
                onJumpToQueue={jumpToQueue}
            />

            {chatMode === 'planning' && latestPlanText && (
                <div className="px-3 pb-1 pt-2">
                    <div className="flex justify-end">
                        <button
                            type="button"
                            onClick={handleImplementPlan}
                            disabled={loading || !canUseAi}
                            className="inline-flex items-center rounded-md border border-emerald-500/30 bg-emerald-500/12 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.14em] text-emerald-200 transition-colors hover:border-emerald-400/50 hover:bg-emerald-500/20 disabled:cursor-not-allowed disabled:opacity-40"
                        >
                            {t('chat.implement')}
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
                disabled={!canUseAi}
                prefillRequest={composerPrefill}
                onPrefillConsumed={handleComposerPrefillConsumed}
            />

            {/* API Key Missing Overlay */}
        </div>
    );
};

export const ChatPanel = React.memo(ChatPanelComponent);
