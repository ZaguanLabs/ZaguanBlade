import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, ChevronRight, Terminal } from 'lucide-react';
import type { ChatMessage as ChatMessageType, HookApprovalRequest, ToolActivityState } from '../../types/chat';
import type { StructuredAction } from '../../types/events';
import { ChatMessage } from '../../components/ChatMessage';
import { ProgressIndicator } from '../../components/ProgressIndicator';
import { computeStableChatTimelineRows, deriveChatTimelineRowsFromProjection, type ChatActivity, type ChatWorkEntry, type StableChatTimelineRowsState } from '../../utils/chatTimeline';
import { FloatingJumpToBottomButton } from './FloatingJumpToBottomButton';
import { useChatProjection } from './useChatProjection';

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

function workEntryToneClass(entry: ChatWorkEntry): string {
    if (entry.status === 'executing' || entry.status === 'pending') {
        return 'bg-(--accent-primary)';
    }
    if (entry.tone === 'error') {
        return 'bg-(--accent-error)';
    }
    return 'bg-(--accent-green)';
}

const WorkLogTimelineRow: React.FC<{
    entries: ChatWorkEntry[];
    showDetails: boolean;
    detailsLockedOpen: boolean;
    onToggleDetails: () => void;
}> = React.memo(({ entries, showDetails, detailsLockedOpen, onToggleDetails }) => {
    const [isExpanded, setIsExpanded] = useState(false);
    const visibleEntries = isExpanded ? entries : entries.slice(0, 4);
    const hiddenCount = entries.length - visibleEntries.length;

    return (
        <div className="px-4 pb-2">
            <div className="ml-8 rounded-xl border border-(--border-subtle) bg-(--bg-surface)/45 px-3 py-2">
                <button
                    type="button"
                    className="flex w-full items-center justify-between gap-3 text-left"
                    onClick={() => setIsExpanded((value) => !value)}
                >
                    <span className="flex min-w-0 items-center gap-2">
                        <Terminal className="h-3.5 w-3.5 shrink-0 text-(--fg-tertiary)" />
                        <span className="text-[9px] font-semibold uppercase tracking-[0.16em] text-(--fg-tertiary)">
                            Work log ({entries.length})
                        </span>
                        {!isExpanded && (
                            <span className="min-w-0 truncate text-[10px] text-(--fg-secondary)">
                                {entries.slice(0, 2).map((entry) => entry.label).join(', ')}
                            </span>
                        )}
                    </span>
                    {isExpanded ? (
                        <ChevronDown className="h-3.5 w-3.5 shrink-0 text-(--fg-tertiary)" />
                    ) : (
                        <ChevronRight className="h-3.5 w-3.5 shrink-0 text-(--fg-tertiary)" />
                    )}
                </button>
                {(isExpanded || entries.length > 4) && (
                    <div className="mt-1.5 space-y-1">
                        {visibleEntries.map((entry) => (
                            <div key={entry.id} className="flex min-w-0 items-center gap-2 rounded-md px-1 py-0.5">
                                <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${workEntryToneClass(entry)}`} />
                                <span className="shrink-0 text-[10px] font-medium text-(--fg-secondary)">
                                    {entry.label}
                                </span>
                                {entry.detail && (
                                    <span className="min-w-0 truncate text-[10px] font-mono text-(--fg-tertiary)" title={entry.detail}>
                                        {entry.detail}
                                    </span>
                                )}
                            </div>
                        ))}
                        {!isExpanded && hiddenCount > 0 && (
                            <div className="px-1 text-[10px] text-(--fg-tertiary)">
                                +{hiddenCount} more
                            </div>
                        )}
                    </div>
                )}
                <div className="mt-1.5 flex items-center justify-end">
                    <button
                        type="button"
                        className="rounded-md px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.12em] text-(--fg-tertiary) transition-colors hover:text-(--fg-secondary) disabled:cursor-default disabled:opacity-60"
                        onClick={onToggleDetails}
                        disabled={detailsLockedOpen}
                    >
                        {detailsLockedOpen ? 'Details visible' : showDetails ? 'Hide details' : 'Show details'}
                    </button>
                </div>
            </div>
        </div>
    );
});

WorkLogTimelineRow.displayName = 'WorkLogTimelineRow';

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
    const stableRowsStateRef = useRef<StableChatTimelineRowsState>({ byKey: new Map(), rows: [] });
    const projection = useChatProjection(messages, chatActivities);
    const rows = useMemo(
        () => {
            const rawRows = deriveChatTimelineRowsFromProjection(projection, loading, pendingActions, pendingApprovalRequest);
            const stableRowsState = computeStableChatTimelineRows(rawRows, stableRowsStateRef.current);
            stableRowsStateRef.current = stableRowsState;
            return stableRowsState.rows;
        },
        [loading, pendingActions, pendingApprovalRequest, projection],
    );
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
                            const detailsLockedOpen = row.entries.some((entry) => entry.status === 'executing' || entry.status === 'pending')
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
