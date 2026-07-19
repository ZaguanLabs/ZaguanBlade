import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ChatMessage as ChatMessageType, HookApprovalRequest, QueuedRequest, ToolActivityState } from '../../types/chat';
import type { StructuredAction } from '../../types/events';
import { ChatMessage } from '../../components/ChatMessage';
import { ProgressIndicator } from '../../components/ProgressIndicator';
import {
    estimateChatRowHeight,
    findFirstUnvirtualizedChatRowIndex,
    type ChatActivity,
    type DerivedChatMessageRow,
} from '../../utils/chatTimeline';
import { FloatingJumpToBottomButton } from './FloatingJumpToBottomButton';
import { FloatingCommandApprovalPill } from './FloatingCommandApprovalPill';
import { useChatTimelineRows } from './useChatTimelineRows';
import { isNearChatBottom, shouldDetachChatAutoScrollOnWheel } from '../../utils/chatScroll';
import { useSmoothWheelScroll } from '../../hooks/useSmoothWheelScroll';
import { readDebugFlag } from '../../utils/debugFlags';
import { recordDebugPerf } from '../../utils/debugPerf';
import { computeVisibleVirtualRange, sameVisibleVirtualRange, type VisibleVirtualRange } from '../../utils/chatVirtualization';
import zbladeAppIcon from '../../assets/zblade-app-icon.png';

const FOLLOW_BOTTOM_THRESHOLD_PX = 48;
const DEFERRED_ROW_MINIMUM_COUNT = 24;
const EMPTY_VIRTUAL_RANGE: VisibleVirtualRange = {
    startIndex: 0,
    endIndex: 0,
    topSpacerHeight: 0,
    bottomSpacerHeight: 0,
};

interface ResearchProgress {
    message: string;
    stage: string;
    percent: number;
    isActive: boolean;
}

interface ChatViewportProps {
    messages: ChatMessageType[];
    loading: boolean;
    pendingActions: StructuredAction[] | null;
    pendingApprovalRequest: HookApprovalRequest | null;
    toolActivity?: ToolActivityState | null;
    chatActivities?: ChatActivity[];
    researchProgress?: ResearchProgress | null;
    onApproveCommand?: () => void;
    onSkipCommand?: () => void;
    onApproveSingleCommand?: (toolCallId: string) => void;
    onSkipSingleCommand?: (toolCallId: string) => void;
    onApproveApprovalRequest?: () => void;
    onDenyApprovalRequest?: () => void;
    onUndoTool?: (toolCallId: string) => void;
    onStopCommand?: (toolCallId: string) => void;
    onOpenFile?: (path: string) => void;
    workspaceRoot?: string | null;
    onEditLastUserMessage?: () => Promise<QueuedRequest | null>;
}

export const ChatViewport: React.FC<ChatViewportProps> = ({
    messages,
    loading,
    pendingActions,
    pendingApprovalRequest,
    toolActivity,
    chatActivities = [],
    researchProgress,
    onApproveCommand,
    onSkipCommand,
    onApproveSingleCommand,
    onSkipSingleCommand,
    onApproveApprovalRequest,
    onDenyApprovalRequest,
    onUndoTool,
    onStopCommand,
    onOpenFile,
    workspaceRoot,
    onEditLastUserMessage,
}) => {
    const { t } = useTranslation();
    recordDebugPerf('ChatViewport.render');
    const compactEmptyStatesV1 = readDebugFlag('compactEmptyStatesV1');
    const scrollRef = useRef<HTMLDivElement>(null);
    const contentRef = useRef<HTMLDivElement>(null);
    const bottomRef = useRef<HTMLDivElement>(null);
    const [scrollMode, setScrollMode] = useState<'following' | 'detached'>('following');
    const scrollModeRef = useRef<'following' | 'detached'>('following');
    const isUserAtBottomRef = useRef(true);
    const isBottomSentinelVisibleRef = useRef(true);
    const previousFirstMessageIdRef = useRef<string | undefined>(undefined);
    const previousMessageCountRef = useRef(0);
    const scrollTopRef = useRef(0);
    const viewportHeightRef = useRef(0);
    const virtualizedRowOffsetsRef = useRef<number[]>([]);
    const virtualizedRowHeightsRef = useRef<number[]>([]);
    const totalVirtualizedHeightRef = useRef(0);
    const visibleRangeFrameRef = useRef<number | null>(null);
    const [smoothScrollResetKey, setSmoothScrollResetKey] = useState(0);
    const [viewportMetrics, setViewportMetrics] = useState({ width: 0, height: 0 });
    const [visibleVirtualRange, setVisibleVirtualRange] = useState<VisibleVirtualRange>(EMPTY_VIRTUAL_RANGE);
    const { rows } = useChatTimelineRows({
        messages,
        activities: chatActivities,
        loading,
        pendingActions,
        pendingApprovalRequest,
    });
    const messageRows = useMemo(
        () => rows.filter((row): row is DerivedChatMessageRow => row.kind === 'message'),
        [rows],
    );
    const firstLiveMessageRowIndex = useMemo(
        () => findFirstUnvirtualizedChatRowIndex(messageRows, loading),
        [loading, messageRows],
    );
    const shouldVirtualizeRows = messageRows.length >= DEFERRED_ROW_MINIMUM_COUNT;
    const virtualizedMessageRows = useMemo(() => {
        if (!shouldVirtualizeRows) {
            return [];
        }
        return messageRows.slice(0, firstLiveMessageRowIndex);
    }, [firstLiveMessageRowIndex, messageRows, shouldVirtualizeRows]);
    const liveMessageRows = useMemo(() => {
        if (!shouldVirtualizeRows) {
            return messageRows;
        }
        return messageRows.slice(firstLiveMessageRowIndex);
    }, [firstLiveMessageRowIndex, messageRows, shouldVirtualizeRows]);
    const pendingRunCommandId = useMemo(() => {
        for (const row of messageRows) {
            if (row.pendingActions) {
                const runCommand = row.pendingActions.find(
                    (action: StructuredAction) => action.actionKind === 'command' || action.is_generic_tool === false
                );
                if (runCommand) {
                    return runCommand.id;
                }
            }
        }
        return null;
    }, [messageRows]);
    const hasPendingRunCommand = pendingRunCommandId !== null;
    const virtualizedRowHeights = useMemo(
        () => virtualizedMessageRows.map((row) => estimateChatRowHeight(row, { viewportWidthPx: viewportMetrics.width })),
        [viewportMetrics.width, virtualizedMessageRows],
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
        [virtualizedRowHeights],
    );
    const visibleVirtualRows = useMemo(
        () => virtualizedMessageRows.slice(visibleVirtualRange.startIndex, visibleVirtualRange.endIndex),
        [virtualizedMessageRows, visibleVirtualRange.endIndex, visibleVirtualRange.startIndex],
    );
    const activeMessage = rows.find((row) => row.kind === 'message' && row.isActive)?.message;
    const lastUserMessageId = [...messages].reverse().find((message) => message.role === 'User')?.id;
    const firstMessageId = messages[0]?.id;
    const lastMessage = messages[messages.length - 1];
    const messageCount = messages.length;
    const streamingSignature = useMemo(() => {
        if (!activeMessage) {
            return '';
        }

        return [
            activeMessage.id,
            activeMessage.content.length,
            activeMessage.reasoning?.length ?? 0,
            activeMessage.blocks?.length ?? 0,
            activeMessage.streaming?.seq ?? '',
        ].join('|');
    }, [activeMessage]);
    const showProgressIndicator = Boolean(researchProgress?.isActive);
    const showPendingResponse = loading && messages[messages.length - 1]?.role !== 'Assistant' && !showProgressIndicator;

    const scheduleVisibleVirtualRangeUpdate = useCallback((scrollTop?: number, immediate = false) => {
        if (typeof scrollTop === 'number') {
            scrollTopRef.current = scrollTop;
        }

        if (visibleRangeFrameRef.current !== null) {
            return;
        }

        const runUpdate = () => {
            visibleRangeFrameRef.current = null;
            recordDebugPerf('ChatViewport.visibleRangeUpdate');
            const nextRange = computeVisibleVirtualRange(
                scrollTopRef.current,
                viewportHeightRef.current,
                virtualizedRowOffsetsRef.current,
                virtualizedRowHeightsRef.current,
                totalVirtualizedHeightRef.current,
            );
            setVisibleVirtualRange((currentRange) => sameVisibleVirtualRange(currentRange, nextRange) ? currentRange : nextRange);
        };

        if (immediate) {
            runUpdate();
            return;
        }

        visibleRangeFrameRef.current = requestAnimationFrame(runUpdate);
    }, []);

    const setStableScrollMode = useCallback((nextMode: 'following' | 'detached') => {
        if (scrollModeRef.current === nextMode) {
            return;
        }
        scrollModeRef.current = nextMode;
        if (nextMode === 'following') {
            setSmoothScrollResetKey((value) => value + 1);
        }
        setScrollMode(nextMode);
    }, []);

    const getDistanceFromBottom = useCallback((element: HTMLDivElement) => {
        return Math.max(0, element.scrollHeight - element.scrollTop - element.clientHeight);
    }, []);

    const syncBottomState = useCallback((element: HTMLDivElement) => {
        const isAtBottom = isNearChatBottom(
            element.scrollHeight,
            element.scrollTop,
            element.clientHeight,
            FOLLOW_BOTTOM_THRESHOLD_PX,
        );

        isUserAtBottomRef.current = isAtBottom;
        if (isAtBottom) {
            isBottomSentinelVisibleRef.current = true;
        }
        setStableScrollMode(isAtBottom ? 'following' : 'detached');
    }, [setStableScrollMode]);

    const scrollToBottom = useCallback((attachFollow = true) => {
        const element = scrollRef.current;
        if (!element) {
            return;
        }

        element.scrollTop = element.scrollHeight;
        scrollTopRef.current = element.scrollTop;
        scheduleVisibleVirtualRangeUpdate(element.scrollTop, true);

        if (attachFollow) {
            isUserAtBottomRef.current = true;
            isBottomSentinelVisibleRef.current = true;
            setStableScrollMode('following');
        }
    }, [setStableScrollMode, scheduleVisibleVirtualRangeUpdate]);

    const scrollToPendingCommand = useCallback(() => {
        const element = scrollRef.current;
        if (!element || !pendingRunCommandId) {
            return;
        }

        const target = element.querySelector(`[data-tool-call-id="${CSS.escape(pendingRunCommandId)}"]`);
        if (target) {
            target.scrollIntoView({ behavior: 'smooth', block: 'center' });
            // After scrolling, re-attach follow mode so future messages scroll naturally
            setStableScrollMode('following');
        }
    }, [pendingRunCommandId, setStableScrollMode]);

    const handleScroll = useCallback((event: React.UIEvent<HTMLDivElement>) => {
        scrollTopRef.current = event.currentTarget.scrollTop;
        scheduleVisibleVirtualRangeUpdate(event.currentTarget.scrollTop);
        syncBottomState(event.currentTarget);
    }, [scheduleVisibleVirtualRangeUpdate, syncBottomState]);

    const handleWheel = useCallback((event: React.WheelEvent<HTMLDivElement>) => {
        const element = scrollRef.current;
        if (!element) {
            return;
        }

        if (shouldDetachChatAutoScrollOnWheel(event.deltaY, element.scrollTop)) {
            isUserAtBottomRef.current = false;
            isBottomSentinelVisibleRef.current = false;
            setStableScrollMode('detached');
        }
    }, [setStableScrollMode]);
    const handleSmoothWheel = useSmoothWheelScroll<HTMLDivElement>(handleWheel, {
        disabled: scrollMode === 'following',
        resetKey: smoothScrollResetKey,
    });

    useEffect(() => {
        virtualizedRowOffsetsRef.current = virtualizedRowOffsets;
        virtualizedRowHeightsRef.current = virtualizedRowHeights;
        totalVirtualizedHeightRef.current = totalVirtualizedHeight;
        viewportHeightRef.current = viewportMetrics.height;
        scheduleVisibleVirtualRangeUpdate(scrollTopRef.current);
    }, [scheduleVisibleVirtualRangeUpdate, totalVirtualizedHeight, viewportMetrics.height, virtualizedRowHeights, virtualizedRowOffsets]);

    useEffect(() => {
        if (virtualizedMessageRows.length > 0) {
            recordDebugPerf('ChatViewport.virtualizedRows', virtualizedMessageRows.length);
        }
        if (visibleVirtualRows.length > 0) {
            recordDebugPerf('ChatViewport.mountedVirtualRows', visibleVirtualRows.length);
        }
    }, [virtualizedMessageRows.length, visibleVirtualRows.length]);

    useEffect(() => {
        const element = scrollRef.current;
        if (!element) {
            return;
        }

        setViewportMetrics({ width: element.clientWidth, height: element.clientHeight });
        viewportHeightRef.current = element.clientHeight;
        scrollTopRef.current = element.scrollTop;

        if (typeof ResizeObserver === 'undefined') {
            return;
        }

        let frameId: number | null = null;
        const observer = new ResizeObserver(([entry]) => {
            recordDebugPerf('ChatViewport.widthResizeObserver');
            const nextWidth = Math.round(entry?.contentRect.width ?? element.clientWidth);
            const nextHeight = Math.round(entry?.contentRect.height ?? element.clientHeight);
            if (frameId !== null) {
                cancelAnimationFrame(frameId);
            }
            frameId = requestAnimationFrame(() => {
                frameId = null;
                viewportHeightRef.current = nextHeight;
                setViewportMetrics((currentMetrics) => (
                    currentMetrics.width === nextWidth && currentMetrics.height === nextHeight
                        ? currentMetrics
                        : { width: nextWidth, height: nextHeight }
                ));
                scheduleVisibleVirtualRangeUpdate(scrollTopRef.current);
            });
        });

        observer.observe(element);
        return () => {
            observer.disconnect();
            if (frameId !== null) {
                cancelAnimationFrame(frameId);
            }
        };
    }, [scheduleVisibleVirtualRangeUpdate]);

    useEffect(() => {
        const content = contentRef.current;
        if (!content || typeof ResizeObserver === 'undefined') {
            return;
        }

        let frameId: number | null = null;
        const observer = new ResizeObserver(() => {
            recordDebugPerf('ChatViewport.contentResizeObserver');
            if (scrollModeRef.current !== 'following' || !isUserAtBottomRef.current) {
                return;
            }

            if (frameId !== null) {
                cancelAnimationFrame(frameId);
            }
            frameId = requestAnimationFrame(() => {
                frameId = null;
                recordDebugPerf('ChatViewport.contentResizeFollowBottom');
                scrollToBottom(false);
            });
        });

        observer.observe(content);
        return () => {
            observer.disconnect();
            if (frameId !== null) {
                cancelAnimationFrame(frameId);
                frameId = null;
            }
        };
    }, [scrollToBottom]);

    useEffect(() => {
        const sentinel = bottomRef.current;
        const scrollRoot = scrollRef.current;
        if (!sentinel || !scrollRoot || typeof IntersectionObserver === 'undefined') {
            return;
        }

        const observer = new IntersectionObserver(
            ([entry]) => {
                isBottomSentinelVisibleRef.current = entry.isIntersecting;
            },
            {
                root: scrollRoot,
                threshold: 0,
                rootMargin: `0px 0px ${FOLLOW_BOTTOM_THRESHOLD_PX}px 0px`,
            },
        );

        observer.observe(sentinel);
        return () => observer.disconnect();
    }, []);

    useEffect(() => {
        if (messageCount === 0) {
            previousFirstMessageIdRef.current = undefined;
            previousMessageCountRef.current = 0;
            isUserAtBottomRef.current = true;
            isBottomSentinelVisibleRef.current = true;
            setStableScrollMode('following');
            return;
        }

        if (firstMessageId === previousFirstMessageIdRef.current) {
            return;
        }

        previousFirstMessageIdRef.current = firstMessageId;
        previousMessageCountRef.current = messageCount;
        isUserAtBottomRef.current = true;
        isBottomSentinelVisibleRef.current = true;
        setStableScrollMode('following');

        const timer = window.setTimeout(() => scrollToBottom(), 50);
        return () => window.clearTimeout(timer);
    }, [firstMessageId, messageCount, scrollToBottom, setStableScrollMode]);

    useEffect(() => {
        if (messageCount === previousMessageCountRef.current) {
            return;
        }

        previousMessageCountRef.current = messageCount;

        if (lastMessage?.role === 'User' || isUserAtBottomRef.current) {
            const frameId = requestAnimationFrame(() => scrollToBottom());
            return () => cancelAnimationFrame(frameId);
        }
    }, [lastMessage?.role, messageCount, scrollToBottom]);

    useLayoutEffect(() => {
        if (!loading || !isUserAtBottomRef.current || !isBottomSentinelVisibleRef.current) {
            return;
        }

        const element = scrollRef.current;
        if (!element) {
            return;
        }

        if (getDistanceFromBottom(element) > 2) {
            element.scrollTop = element.scrollHeight;
        }
    }, [getDistanceFromBottom, loading, streamingSignature]);

    useEffect(() => () => {
        if (visibleRangeFrameRef.current !== null) {
            cancelAnimationFrame(visibleRangeFrameRef.current);
            visibleRangeFrameRef.current = null;
        }
    }, []);

    const renderMessageRow = useCallback((row: DerivedChatMessageRow) => (
        <ChatMessage
            message={row.message}
            pendingActions={row.pendingActions}
            pendingApprovalRequest={row.pendingApprovalRequest}
            onApproveCommand={row.pendingActions ? onApproveCommand : undefined}
            onSkipCommand={row.pendingActions ? onSkipCommand : undefined}
            onApproveApprovalRequest={row.pendingApprovalRequest ? onApproveApprovalRequest : undefined}
            onDenyApprovalRequest={row.pendingApprovalRequest ? onDenyApprovalRequest : undefined}
            onApproveSingleCommand={row.pendingActions ? onApproveSingleCommand : undefined}
            onSkipSingleCommand={row.pendingActions ? onSkipSingleCommand : undefined}
            isContinued={row.isContinued}
            isActive={row.isActive}
            onUndoTool={onUndoTool}
            onStopCommand={onStopCommand}
            onOpenFile={onOpenFile}
            workspaceRoot={workspaceRoot}
            onEditMessage={row.message.id === lastUserMessageId ? onEditLastUserMessage : undefined}
            showInlineWorkLog={false}
            workDetailsVisible={true}
        />
    ), [
        lastUserMessageId,
        onApproveApprovalRequest,
        onApproveCommand,
        onDenyApprovalRequest,
        onEditLastUserMessage,
        onOpenFile,
        onSkipCommand,
        onApproveSingleCommand,
        onSkipSingleCommand,
        onStopCommand,
        onUndoTool,
        workspaceRoot,
    ]);

    return (
        <div className="relative min-h-0 flex-1">
            <div ref={scrollRef} onScroll={handleScroll} onWheel={handleSmoothWheel} className="h-full overflow-y-auto overscroll-contain [overflow-anchor:none] scrollbar-thin scrollbar-thumb-(--bg-surface-hover) scrollbar-track-transparent">
                <div ref={contentRef} className="mx-auto flex w-full max-w-none flex-col gap-1 px-0.5 py-4 md:px-1">
                    {messages.length === 0 && (
                        compactEmptyStatesV1 ? (
                            <div className="mx-3 mt-3 border-b border-(--separator-subtle) px-1 pb-4 text-left">
                                <div className="flex items-center gap-3">
                                    <img src={zbladeAppIcon} alt="" className="h-8 w-8 object-contain" draggable={false} />
                                    <div className="min-w-0">
                                        <h2 className="text-sm font-semibold text-(--fg-primary)">{t('app.name')}</h2>
                                        <p className="mt-1 truncate text-[11px] text-(--fg-tertiary)">
                                            {workspaceRoot || t('chat.emptyState.intro')}
                                        </p>
                                    </div>
                                </div>
                                <p className="mt-3 max-w-md text-xs leading-5 text-(--fg-secondary)">
                                    {t('chat.emptyState.tip')}
                                </p>
                            </div>
                        ) : (
                            <div className="mx-4 mt-10 rounded-(--panel-radius) border border-(--border-subtle) bg-(--bg-surface)/70 px-6 py-8 text-center shadow-(--panel-shadow)">
                                <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-[calc(var(--panel-radius)*0.9)] border border-[color-mix(in_srgb,var(--accent-ai)_24%,transparent)] bg-[color-mix(in_srgb,var(--accent-ai)_10%,transparent)] shadow-(--shadow-lg)">
                                    <img src={zbladeAppIcon} alt="" className="h-9 w-9 object-contain" draggable={false} />
                                </div>
                                <h2 className="text-sm font-semibold uppercase tracking-[0.24em] text-(--fg-secondary)">{t('app.name')}</h2>
                                <p className="mx-auto mt-3 max-w-md text-sm leading-6 text-(--fg-tertiary)">
                                    {t('chat.emptyState.intro')}
                                </p>
                                <p className="mx-auto mt-3 max-w-md text-sm leading-6 text-(--fg-secondary)">
                                    {t('chat.emptyState.tip')}
                                </p>
                            </div>
                        )
                    )}

                    {visibleVirtualRange.topSpacerHeight > 0 && (
                        <div style={{ height: `${visibleVirtualRange.topSpacerHeight}px` }} />
                    )}

                    {visibleVirtualRows.map((row) => (
                        <div key={row.key} className="chat-offscreen-row">
                            {renderMessageRow(row)}
                        </div>
                    ))}

                    {visibleVirtualRange.bottomSpacerHeight > 0 && (
                        <div style={{ height: `${visibleVirtualRange.bottomSpacerHeight}px` }} />
                    )}

                    {liveMessageRows.map((row) => (
                        <div key={row.key} className={row.isActive ? undefined : 'chat-offscreen-row'}>
                            {renderMessageRow(row)}
                        </div>
                    ))}

                    {showPendingResponse && (
                        <div className="px-4 py-3">
                            <div
                                role="status"
                                aria-live="polite"
                                className="inline-flex max-w-full items-center gap-3 rounded-[calc(var(--panel-radius)*0.9)] border border-[color-mix(in_srgb,var(--accent-ai)_18%,transparent)] bg-(--bg-surface)/70 px-4 py-3 text-[11px] font-medium text-(--fg-secondary)"
                            >
                                <span aria-hidden="true" className="h-2 w-2 rounded-full bg-(--accent-ai)" />
                                <span className="truncate">{toolActivity?.action ?? t('chat.assistantResponding')}</span>
                            </div>
                        </div>
                    )}

                    {showProgressIndicator && researchProgress && (
                        <div className="px-4">
                            <ProgressIndicator progress={researchProgress} />
                        </div>
                    )}

                    <div ref={bottomRef} className="h-4" />
                </div>
            </div>
            <FloatingCommandApprovalPill visible={scrollMode === 'detached' && hasPendingRunCommand} onClick={scrollToPendingCommand} />
            <FloatingJumpToBottomButton visible={scrollMode === 'detached'} onClick={() => scrollToBottom()} />
        </div>
    );
};
