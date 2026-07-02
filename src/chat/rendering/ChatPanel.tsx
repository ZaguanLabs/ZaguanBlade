import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ChatMessage as ChatMessageType, ChatMode, CognitiveInterruptState, ComposerMention, HookApprovalRequest, ImageAttachment, ModelInfo, QueuedRequest, ToolActivityState } from '../../types/chat';
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
import { CognitiveInterruptStrip } from './CognitiveInterruptStrip';

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
    cognitiveInterrupt?: CognitiveInterruptState | null;
    chatActivities?: ChatActivity[];
    activeTodos: TodoItem[];
    queuedRequests: QueuedRequest[];
    deleteQueuedRequest: (index: number) => void;
    editLastUserMessage: () => Promise<QueuedRequest | null>;
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
    cognitiveInterrupt,
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

    const handleNewConversation = useCallback(() => {
        onNewConversation();
        setActiveTab('chat');
    }, [onNewConversation]);

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
            <CognitiveInterruptStrip interrupt={cognitiveInterrupt ?? null} />
            <TaskStrip todos={activeTodos} />
            <QueueStrip requests={queuedRequests} onEditRequest={handleEditQueuedRequest} onDeleteRequest={deleteQueuedRequest} />
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
