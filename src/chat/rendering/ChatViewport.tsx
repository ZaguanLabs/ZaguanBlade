import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ChatMessage as ChatMessageType, HookApprovalRequest, ToolActivityState } from '../../types/chat';
import type { StructuredAction } from '../../types/events';
import { ChatMessage } from '../../components/ChatMessage';
import { ProgressIndicator } from '../../components/ProgressIndicator';
import type { ChatActivity } from '../../utils/chatTimeline';
import { FloatingJumpToBottomButton } from './FloatingJumpToBottomButton';
import { useChatTimelineRows } from './useChatTimelineRows';
import { WorkLogTimelineRow } from './WorkLogTimelineRow';

const FOLLOW_BOTTOM_THRESHOLD_PX = 48;
const DETACH_BOTTOM_THRESHOLD_PX = 140;

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
}) => {
    const { t } = useTranslation();
    const scrollRef = useRef<HTMLDivElement>(null);
    const bottomRef = useRef<HTMLDivElement>(null);
    const [scrollMode, setScrollMode] = useState<'following' | 'detached'>('following');
    const [visibleWorkDetailsByMessageId, setVisibleWorkDetailsByMessageId] = useState<Record<string, boolean>>({});
    const scrollModeRef = useRef<'following' | 'detached'>('following');
    const { rows, activeWorkState } = useChatTimelineRows({
        messages,
        activities: chatActivities,
        loading,
        pendingActions,
        pendingApprovalRequest,
    });
    const activeMessage = rows.find((row) => row.kind === 'message' && row.isActive)?.message;
    const showProgressIndicator = Boolean(researchProgress?.isActive);
    const showPendingResponse = loading && messages[messages.length - 1]?.role !== 'Assistant' && !showProgressIndicator;

    const setStableScrollMode = useCallback((nextMode: 'following' | 'detached') => {
        if (scrollModeRef.current === nextMode) {
            return;
        }
        scrollModeRef.current = nextMode;
        setScrollMode(nextMode);
    }, []);

    const getDistanceFromBottom = useCallback(() => {
        const element = scrollRef.current;
        if (!element) {
            return 0;
        }
        return Math.max(0, element.scrollHeight - element.scrollTop - element.clientHeight);
    }, []);

    const handleScroll = useCallback(() => {
        const distanceFromBottom = getDistanceFromBottom();
        if (scrollModeRef.current === 'following' && distanceFromBottom > DETACH_BOTTOM_THRESHOLD_PX) {
            setStableScrollMode('detached');
            return;
        }
        if (scrollModeRef.current === 'detached' && distanceFromBottom <= FOLLOW_BOTTOM_THRESHOLD_PX) {
            setStableScrollMode('following');
        }
    }, [getDistanceFromBottom, setStableScrollMode]);

    const scrollToBottom = useCallback(() => {
        const element = scrollRef.current;
        if (!element) {
            return;
        }
        bottomRef.current?.scrollIntoView({ block: 'end' });
        requestAnimationFrame(() => {
            element.scrollTop = element.scrollHeight;
        });
        setStableScrollMode('following');
    }, [setStableScrollMode]);

    const toggleWorkDetails = useCallback((messageId: string) => {
        setVisibleWorkDetailsByMessageId((previous) => ({
            ...previous,
            [messageId]: !previous[messageId],
        }));
    }, []);

    useEffect(() => {
        if (scrollMode !== 'following') {
            return;
        }
        const frame = requestAnimationFrame(() => {
            bottomRef.current?.scrollIntoView({ block: 'end' });
        });
        return () => cancelAnimationFrame(frame);
    }, [activeMessage?.content, activeMessage?.reasoning, activeMessage?.streaming?.seq, rows.length, scrollMode]);

    return (
        <div className="relative min-h-0 flex-1">
            <div ref={scrollRef} onScroll={handleScroll} className="h-full overflow-y-auto scrollbar-thin scrollbar-thumb-zinc-800 scrollbar-track-transparent">
                <div className="mx-auto flex w-full max-w-none flex-col gap-0.5 px-0.5 py-4 md:px-1">
                    {messages.length === 0 && (
                        <div className="mx-4 mt-10 rounded-2xl border border-(--border-subtle) bg-(--bg-surface)/70 px-6 py-8 text-center shadow-[0_24px_80px_rgba(0,0,0,0.25)]">
                            <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-2xl border border-emerald-500/20 bg-emerald-500/10 text-2xl shadow-[0_0_40px_rgba(16,185,129,0.15)]">
                                BL
                            </div>
                            <h2 className="text-sm font-semibold uppercase tracking-[0.24em] text-(--fg-secondary)">{t('app.name')}</h2>
                            <p className="mx-auto mt-3 max-w-md text-sm leading-6 text-(--fg-tertiary)">
                                {t('chat.emptyState.intro')}
                            </p>
                            <p className="mx-auto mt-3 max-w-md text-sm leading-6 text-(--fg-secondary)">
                                {t('chat.emptyState.tip')}
                            </p>
                        </div>
                    )}

                    {rows.map((row) => {
                        if (row.kind === 'work_log') {
                            const detailsLockedOpen = activeWorkState.activeMessageIds.has(row.message.id || row.key)
                                || !!row.pendingApprovalRequest
                                || !!row.pendingActions?.length;
                            const messageId = row.message.id || row.key;
                            return (
                                <WorkLogTimelineRow
                                    key={row.key}
                                    entries={row.entries}
                                    showDetails={!!visibleWorkDetailsByMessageId[messageId]}
                                    detailsLockedOpen={detailsLockedOpen}
                                    onToggleDetails={() => toggleWorkDetails(messageId)}
                                />
                            );
                        }

                        const messageId = row.message.id || row.key;
                        return (
                            <ChatMessage
                                key={row.key}
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
                                showInlineWorkLog={false}
                                workDetailsVisible={!!visibleWorkDetailsByMessageId[messageId]}
                                onToggleWorkDetails={() => toggleWorkDetails(messageId)}
                            />
                        );
                    })}

                    {showPendingResponse && (
                        <div className="px-4 py-3">
                            <div className="inline-flex max-w-full items-center gap-3 rounded-2xl border border-emerald-500/15 bg-(--bg-surface)/70 px-4 py-3 text-[11px] font-medium text-(--fg-secondary)">
                                <span className="h-2 w-2 rounded-full bg-emerald-400/80" />
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
            <FloatingJumpToBottomButton visible={scrollMode === 'detached'} onClick={scrollToBottom} />
        </div>
    );
};
