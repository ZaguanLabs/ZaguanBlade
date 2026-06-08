import React, { useCallback, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ChatMessage as ChatMessageType, HookApprovalRequest, QueuedRequest, ToolActivityState } from '../../types/chat';
import type { StructuredAction } from '../../types/events';
import { ChatMessage } from '../../components/ChatMessage';
import { ProgressIndicator } from '../../components/ProgressIndicator';
import type { ChatActivity } from '../../utils/chatTimeline';
import { FloatingJumpToBottomButton } from './FloatingJumpToBottomButton';
import { useChatTimelineRows } from './useChatTimelineRows';
import { shouldDetachChatAutoScrollOnWheel } from '../../utils/chatScroll';
import { useSmoothWheelScroll } from '../../hooks/useSmoothWheelScroll';
import { readDebugFlag } from '../../utils/debugFlags';
import zbladeAppIcon from '../../assets/zblade-app-icon.png';

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
    const compactEmptyStatesV1 = readDebugFlag('compactEmptyStatesV1');
    const scrollRef = useRef<HTMLDivElement>(null);
    const bottomRef = useRef<HTMLDivElement>(null);
    const [scrollMode, setScrollMode] = useState<'following' | 'detached'>('following');
    const scrollModeRef = useRef<'following' | 'detached'>('following');
    const wheelDetachedRef = useRef(false);
    const [smoothScrollResetKey, setSmoothScrollResetKey] = useState(0);
    const { rows } = useChatTimelineRows({
        messages,
        activities: chatActivities,
        loading,
        pendingActions,
        pendingApprovalRequest,
    });
    const activeMessage = rows.find((row) => row.kind === 'message' && row.isActive)?.message;
    const lastUserMessageId = [...messages].reverse().find((message) => message.role === 'User')?.id;
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
        if (scrollModeRef.current === 'detached' && distanceFromBottom <= FOLLOW_BOTTOM_THRESHOLD_PX && !wheelDetachedRef.current) {
            setStableScrollMode('following');
        }
    }, [getDistanceFromBottom, setStableScrollMode]);

    const handleWheel = useCallback((event: React.WheelEvent<HTMLDivElement>) => {
        const element = scrollRef.current;
        if (!element) {
            return;
        }

        if (event.deltaY > 0) {
            wheelDetachedRef.current = false;
        }

        if (shouldDetachChatAutoScrollOnWheel(event.deltaY, element.scrollTop)) {
            wheelDetachedRef.current = true;
            setStableScrollMode('detached');
        }
    }, [setStableScrollMode]);
    const handleSmoothWheel = useSmoothWheelScroll<HTMLDivElement>(handleWheel, {
        disabled: scrollMode === 'following',
        resetKey: smoothScrollResetKey,
    });

    const scrollToBottom = useCallback(() => {
        const element = scrollRef.current;
        if (!element) {
            return;
        }
        setSmoothScrollResetKey((value) => value + 1);
        wheelDetachedRef.current = false;
        element.scrollTop = element.scrollHeight;
        setStableScrollMode('following');
    }, [setStableScrollMode]);

    useLayoutEffect(() => {
        if (scrollMode !== 'following') {
            return;
        }
        const element = scrollRef.current;
        if (!element) {
            return;
        }
        element.scrollTop = element.scrollHeight;
    }, [activeMessage?.content, activeMessage?.reasoning, activeMessage?.streaming?.seq, rows.length, scrollMode]);

    return (
        <div className="relative min-h-0 flex-1">
            <div ref={scrollRef} onScroll={handleScroll} onWheel={handleSmoothWheel} className="h-full overflow-y-auto overscroll-contain [overflow-anchor:none] scrollbar-thin scrollbar-thumb-(--bg-surface-hover) scrollbar-track-transparent">
                <div className="mx-auto flex w-full max-w-none flex-col gap-1 px-0.5 py-4 md:px-1">
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

                    {rows.map((row) => {
                        if (row.kind === 'work_log') {
                            return null;
                        }

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
                                workspaceRoot={workspaceRoot}
                                onEditMessage={row.message.id === lastUserMessageId ? onEditLastUserMessage : undefined}
                                showInlineWorkLog={false}
                                workDetailsVisible={true}
                            />
                        );
                    })}

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
            <FloatingJumpToBottomButton visible={scrollMode === 'detached'} onClick={scrollToBottom} />
        </div>
    );
};
