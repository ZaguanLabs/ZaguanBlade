import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ChatMessage as ChatMessageType, ChatMode, ComposerMention, HookApprovalRequest, ImageAttachment, ModelInfo, QueuedRequest, ToolActivityState } from '../../types/chat';
import type { StructuredAction, TodoItem } from '../../types/events';
import type { UncommittedChange } from '../../types/uncommitted';
import type { ChatActivity } from '../../utils/chatTimeline';
import { GlobalChangeActions } from '../../components/editor/GlobalChangeActions';
import { HistoryTab } from '../../components/HistoryTab';
import { Composer } from '../composer/Composer';
import { useHistory } from '../../hooks/useHistory';
import { useCommandExecution } from '../../hooks/useCommandExecution';
import { ChatHeader } from './ChatHeader';
import { ChatViewport } from './ChatViewport';
import { QueueStrip } from './QueueStrip';
import { TaskStrip } from './TaskStrip';

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
    workspaceRoot?: string | null;
    uncommittedChanges: UncommittedChange[];
    onAcceptAllChanges: () => void;
    onRejectAllChanges: () => void;
    toolActivity?: ToolActivityState | null;
    chatActivities?: ChatActivity[];
    activeTodos: TodoItem[];
    queuedRequests: QueuedRequest[];
    deleteQueuedRequest: (index: number) => void;
    editLastUserMessage: () => Promise<QueuedRequest | null>;
    onImplementPlan: (planText: string) => void;
}

type CloudErrorRecovery = {
    messageKey: string;
    primaryLabelKey: string;
    primaryHref: string;
};

function classifyCloudError(error: string | null): CloudErrorRecovery | null {
    if (!error) {
        return null;
    }

    const lower = error.toLowerCase();
    const normalized = lower.replace(/[_-]/g, ' ');
    const mentionsCredits = normalized.includes('credit') || lower.includes('insufficient_credits');
    const isCreditFailure = lower.includes('insufficient_credits')
        || normalized.includes('insufficient credits')
        || (mentionsCredits && (
            normalized.includes('exhaust')
            || normalized.includes('balance')
            || normalized.includes('not enough')
            || normalized.includes('402')
        ));

    if (isCreditFailure) {
        return {
            messageKey: 'chat.cloudError.credits',
            primaryLabelKey: 'chat.cloudError.buyCredits',
            primaryHref: 'https://zaguanai.com/dashboard/usage',
        };
    }

    const isAccountFailure = normalized.includes('invalid api key')
        || normalized.includes('api key has expired')
        || normalized.includes('revoked')
        || normalized.includes('subscription')
        || normalized.includes('unauthorized')
        || normalized.includes('401')
        || normalized.includes('ps live')
        || normalized.includes('ps test');

    if (isAccountFailure) {
        return {
            messageKey: 'chat.cloudError.account',
            primaryLabelKey: 'chat.cloudError.manageAccount',
            primaryHref: 'https://zaguanai.com/dashboard',
        };
    }

    return null;
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
    approveSingleCommand,
    skipSingleCommand,
    respondToApprovalRequest,
    projectId,
    onLoadConversation,
    researchProgress,
    onNewConversation,
    onUndoTool,
    onOpenFile,
    workspaceRoot,
    uncommittedChanges,
    onAcceptAllChanges,
    onRejectAllChanges,
    toolActivity,
    chatActivities,
    activeTodos,
    queuedRequests,
    deleteQueuedRequest,
    editLastUserMessage,
    onImplementPlan,
}) => {
    const { t } = useTranslation();
    const [activeTab, setActiveTab] = useState<'chat' | 'history'>('chat');
    const [composerPrefill, setComposerPrefill] = useState<QueuedRequest | null>(null);
    const { loadConversation } = useHistory();
    const { stopCommandExecution } = useCommandExecution();
    const canUseAi = models.length > 0;
    const cloudErrorRecovery = useMemo(() => classifyCloudError(error), [error]);

    const handleNewConversation = useCallback(() => {
        onNewConversation();
        setActiveTab('chat');
    }, [onNewConversation]);

    const openLocalAiSettings = useCallback(() => {
        document.dispatchEvent(new CustomEvent('open-settings', { detail: { section: 'localai' } }));
    }, []);

    const handleEditQueuedRequest = useCallback((index: number) => {
        const request = queuedRequests[index];
        if (!request) {
            return;
        }
        setComposerPrefill(request);
        deleteQueuedRequest(index);
    }, [deleteQueuedRequest, queuedRequests]);

    const handleEditLastUserMessage = useCallback(async () => {
        const request = await editLastUserMessage();
        if (request) {
            setComposerPrefill(request);
        }
        return request;
    }, [editLastUserMessage]);

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

    return (
        <div
            className="flex h-full flex-col bg-(--bg-app) font-sans text-(--fg-primary)"
            data-font-zoom-scope="chat"
            style={{ '--markdown-font-size': 'var(--chat-content-font-size, 13px)' } as React.CSSProperties}
        >
            <ChatHeader activeTab={activeTab} onTabChange={setActiveTab} onNewConversation={handleNewConversation} />

            {activeTab === 'chat' ? (
                <ChatViewport
                    messages={messages}
                    loading={loading}
                    pendingActions={pendingActions}
                    pendingApprovalRequest={pendingApprovalRequest}
                    toolActivity={toolActivity}
                    chatActivities={chatActivities}
                    researchProgress={researchProgress}
                    onApproveCommand={() => approveToolDecision('approve_once')}
                    onSkipCommand={() => approveToolDecision('reject')}
                    onApproveSingleCommand={approveSingleCommand}
                    onSkipSingleCommand={skipSingleCommand}
                    onApproveApprovalRequest={() => respondToApprovalRequest(true)}
                    onDenyApprovalRequest={() => respondToApprovalRequest(false)}
                    onUndoTool={onUndoTool}
                    onStopCommand={stopCommandExecution}
                    onOpenFile={onOpenFile}
                    workspaceRoot={workspaceRoot}
                    onEditLastUserMessage={handleEditLastUserMessage}
                />
            ) : (
                <HistoryTab
                    projectId={projectId}
                    onSelectConversation={async (sessionId) => {
                        const conversationMessages = await loadConversation(sessionId);
                        onLoadConversation(conversationMessages);
                        setActiveTab('chat');
                    }}
                />
            )}

            <GlobalChangeActions
                changes={uncommittedChanges}
                onAcceptAll={onAcceptAllChanges}
                onRejectAll={onRejectAllChanges}
            />
            <TaskStrip todos={activeTodos} />
            <QueueStrip requests={queuedRequests} onEditRequest={handleEditQueuedRequest} onDeleteRequest={deleteQueuedRequest} />
            {error && (
                <div className="px-3 pt-2">
                    <div
                        role="alert"
                        className="flex flex-col gap-2 rounded-[calc(var(--panel-radius)*0.65)] border border-[color-mix(in_srgb,var(--state-danger)_34%,transparent)] bg-[color-mix(in_srgb,var(--state-danger)_8%,var(--bg-surface))] px-3 py-2 text-xs shadow-(--shadow-sm)"
                    >
                        <div className="min-w-0">
                            <div className="mb-1 text-[11px] font-semibold uppercase tracking-[0.14em] text-(--state-danger)">
                                {t('chat.cloudError.title')}
                            </div>
                            <p className="m-0 break-words text-(--fg-secondary)">{error}</p>
                        </div>
                        {cloudErrorRecovery && (
                            <div className="flex flex-col gap-2 border-t border-[color-mix(in_srgb,var(--state-danger)_24%,transparent)] pt-2 sm:flex-row sm:items-center sm:justify-between">
                                <p className="m-0 min-w-0 text-(--fg-primary)">
                                    {t(cloudErrorRecovery.messageKey)}
                                </p>
                                <div className="flex flex-wrap items-center gap-2">
                                    <a
                                        href={cloudErrorRecovery.primaryHref}
                                        target="_blank"
                                        rel="noreferrer"
                                        className="inline-flex h-7 items-center rounded-[calc(var(--panel-radius)*0.45)] bg-(--accent-ai) px-2.5 text-[11px] font-semibold text-(--fg-bright) transition-[filter] hover:brightness-110"
                                    >
                                        {t(cloudErrorRecovery.primaryLabelKey)}
                                    </a>
                                    <button
                                        type="button"
                                        onClick={openLocalAiSettings}
                                        className="inline-flex h-7 items-center rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-subtle) bg-(--bg-app)/70 px-2.5 text-[11px] font-medium text-(--fg-secondary) transition-colors hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)"
                                    >
                                        {t('chat.cloudError.useLocalAi')}
                                    </button>
                                </div>
                            </div>
                        )}
                    </div>
                </div>
            )}
            {chatMode === 'planning' && latestPlanText && (
                <div className="px-3 pb-1 pt-2">
                    <div className="flex justify-end">
                        <button
                            type="button"
                            onClick={handleImplementPlan}
                            disabled={loading || !canUseAi}
                            className="inline-flex items-center rounded-[calc(var(--panel-radius)*0.45)] border border-(--accent-mention)/30 bg-[color-mix(in_srgb,var(--accent-mention)_12%,transparent)] px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.14em] text-(--accent-mention) transition-colors hover:border-(--accent-mention)/50 hover:bg-[color-mix(in_srgb,var(--accent-mention)_20%,transparent)] disabled:cursor-not-allowed disabled:opacity-40"
                        >
                            {t('chat.implement')}
                        </button>
                    </div>
                </div>
            )}
            <Composer
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
                onPrefillConsumed={() => setComposerPrefill(null)}
            />
        </div>
    );
};

export const ChatPanel = React.memo(ChatPanelComponent);
