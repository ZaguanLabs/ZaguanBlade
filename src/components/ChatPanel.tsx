import React, { useEffect, useRef, useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, X } from 'lucide-react';
import { useCommandExecution } from '../hooks/useCommandExecution';
import { useHistory } from '../hooks/useHistory';
import type { ChatMessage as ChatMessageType, ModelInfo } from '../types/chat';
import type { Change } from '../types/change';
import type { StructuredAction } from '../types/events';
import { ChatMessage } from './ChatMessage';
import { ChatTabBar } from './ChatTabBar';
import { CommandCenter } from './CommandCenter';
import { HistoryTab } from './HistoryTab';
import { ChatTerminal } from './ChatTerminal';

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
    sendMessage: (text: string) => void;
    stopGeneration: () => void;
    models: ModelInfo[];
    selectedModelId: string;
    setSelectedModelId: (modelId: string) => void;
    pendingActions: StructuredAction[] | null;
    approveToolDecision: (decision: string) => void;
    pendingChanges: Change[];
    approveAllChanges: () => void;
    rejectChange: (changeId: string) => void;
    userId: string;
    projectId: string;
    onLoadConversation: (messages: ChatMessageType[]) => void;
    researchProgress?: ResearchProgress | null;
}

export const ChatPanel: React.FC<ChatPanelProps> = ({
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
    pendingChanges,
    approveAllChanges,
    rejectChange,
    userId,
    projectId,
    onLoadConversation,
    researchProgress,
}) => {
    const { t } = useTranslation();
    const { executions, handleCommandComplete } = useCommandExecution();
    const { loadConversation } = useHistory();
    const messagesEndRef = useRef<HTMLDivElement>(null);
    const isUserAtBottomRef = useRef(true);
    const [activeTab, setActiveTab] = useState<'chat' | 'history'>('chat');

    // Auto-scroll logic
    useEffect(() => {
        // If we just sent a message (loading became true and it's a new user message), force scroll
        // Or if we are already at bottom, keep scrolling.

        // Check if the last message is User, implies we just sent it -> Force Scroll
        const lastMsg = messages[messages.length - 1];
        const justSent = lastMsg?.role === 'User';

        if (justSent || isUserAtBottomRef.current) {
            messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
        }
    }, [messages, loading, pendingChanges]);

    // Prevent default context menu on empty areas
    const handleContextMenu = useCallback((e: React.MouseEvent) => {
        // Always prevent default to avoid native Tauri menu
        e.preventDefault();
    }, []);

    const handleNewConversation = () => {
        // TODO: Implement new conversation logic (clear current messages)
        console.log('New conversation clicked');
        setActiveTab('chat');
    };

    const handleSelectConversation = useCallback(async (sessionId: string) => {
        try {
            const conversationMessages = await loadConversation(sessionId, userId);
            onLoadConversation(conversationMessages);
            setActiveTab('chat');
        } catch (e) {
            console.error('Failed to load conversation:', e);
        }
    }, [loadConversation, userId, onLoadConversation]);

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
                    className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-zinc-800 scrollbar-track-transparent"
                    onScroll={(e) => {
                        const target = e.target as HTMLDivElement;
                        const isBottom = Math.abs(target.scrollHeight - target.scrollTop - target.clientHeight) < 50;
                        isUserAtBottomRef.current = isBottom;
                    }}
                >
                <div className="max-w-4xl mx-auto py-6">
                    {messages.length === 0 && (
                        <div className="flex flex-col items-center justify-center p-12 text-[var(--fg-tertiary)] text-center space-y-4 select-none">
                            <div className="text-4xl opacity-20 filter grayscale">
                                🗡️
                            </div>
                            <h2 className="text-sm font-medium text-[var(--fg-secondary)] tracking-wide uppercase">System Ready</h2>
                            <p className="max-w-xs text-xs font-mono opacity-50">
                                Awaiting input. Surgical precision engaged.
                            </p>
                        </div>
                    )}

                    {messages.map((msg, idx) => {
                        // Show pending actions on the last assistant message
                        const isLast = idx === messages.length - 1;
                        const isLastAssistant = isLast && msg.role === 'Assistant';
                        const showPendingActions = isLastAssistant && pendingActions && pendingActions.length > 0;

                        // Calculate visual grouping props
                        const prevMsg = idx > 0 ? messages[idx - 1] : null;

                        // Treat "Tool" messages (if any visible) as part of Assistant flow if previous was Assistant
                        // Currently we hide Tool messages in ChatMessage, but if they were shown or if we have
                        // consecutive Assistant messages (split reasoning/tool calls), we group them.
                        const isAssistant = msg.role === 'Assistant';
                        const prevWasAssistant = prevMsg?.role === 'Assistant';

                        // Simple grouping: If current is assistant and previous was assistant
                        const isContinued = isAssistant && prevWasAssistant;

                        // Determine if this message is actively streaming/reasoning
                        // We assume the last message is active if global loading state is true
                        const isActive = isLast && loading;

                        // Find active executions for this message's tool calls
                        const messageExecutions = msg.tool_calls
                            ?.map(tc => executions.get(tc.id))
                            .filter(Boolean) || [];

                        return (
                            <React.Fragment key={idx}>
                                <ChatMessage
                                    message={msg}
                                    pendingActions={showPendingActions ? pendingActions : undefined}
                                    onApproveCommand={showPendingActions ? () => approveToolDecision('approve_once') : undefined}
                                    onSkipCommand={showPendingActions ? () => approveToolDecision('reject') : undefined}
                                    isContinued={isContinued}
                                    isActive={isActive}
                                />

                                {/* Show terminals right after the message they belong to */}
                                {messageExecutions.map((exec) => exec && (
                                    <div key={exec.callId} className="px-4 py-2 pl-[44px]"> {/* Increased indent for continued look */}
                                        <ChatTerminal
                                            commandId={exec.commandId}
                                            command={exec.command}
                                            cwd={exec.cwd}
                                            onComplete={(output, exitCode) => {
                                                handleCommandComplete(exec.callId, output, exitCode);
                                            }}
                                        />
                                    </div>
                                ))}
                            </React.Fragment>
                        );
                    })}

                    {/* Research progress indicator */}
                    {researchProgress?.isActive && (
                        <div className="px-4 py-3 mx-4 my-4 bg-[var(--bg-surface)] border border-[var(--border-subtle)] rounded-md">
                            <div className="flex items-center gap-3">
                                {researchProgress.percent >= 100 ? (
                                    <div className="w-4 h-4 rounded-full bg-green-500 flex items-center justify-center">
                                        <Check className="w-3 h-3 text-white" />
                                    </div>
                                ) : (
                                    <div className="animate-spin w-4 h-4 border-2 border-[var(--accent-primary)] border-t-transparent rounded-full" />
                                )}
                                <div className="flex-1">
                                    <div className="text-sm text-[var(--fg-primary)]">
                                        {researchProgress.message || 'Researching...'}
                                    </div>
                                    <div className="mt-1.5 h-1.5 bg-[var(--bg-app)] rounded-full overflow-hidden">
                                        <div 
                                            className="h-full bg-[var(--accent-primary)] transition-all duration-300"
                                            style={{ width: `${researchProgress.percent}%` }}
                                        />
                                    </div>
                                </div>
                                <div className="text-xs text-[var(--fg-tertiary)]">
                                    {researchProgress.percent}%
                                </div>
                            </div>
                        </div>
                    )}

                    {/* Pending Change Proposals - now shown in editor */}
                    {pendingChanges.length > 0 && (
                        <div className="px-4 py-3 bg-purple-900/20 border border-purple-500/30 rounded-md mx-4">
                            <div className="text-xs text-purple-300 font-semibold mb-1">
                                📝 {pendingChanges.length} change{pendingChanges.length > 1 ? 's' : ''} pending review
                            </div>
                            <div className="text-xs text-[var(--fg-secondary)]">
                                {pendingChanges.map(c => c.path?.split('/').pop() || 'unknown').join(', ')}
                            </div>
                            <div className="text-xs text-[var(--fg-tertiary)] mt-2">
                                Open the file in the editor to review changes, or use the buttons below.
                            </div>
                        </div>
                    )}

                    {error && (
                        <div className="p-3 mx-4 mb-4 bg-red-500/5 border border-red-500/20 text-red-400 rounded-sm text-xs font-mono">
                            ERR: {error}
                        </div>
                    )}

                    {/* We actually don't need this div if we scroll container, but it's useful for 'scrollIntoView' method */}
                    <div ref={messagesEndRef} className="h-4" />
                </div>
            </div>
            ) : (
                <HistoryTab 
                    userId={userId}
                    projectId={projectId}
                    onSelectConversation={handleSelectConversation}
                />
            )}

            {/* Global Accept/Reject All Buttons */}
            {pendingChanges.length > 0 && (
                <div className="shrink-0 px-3 py-2 bg-[var(--bg-panel)] border-t border-[var(--border-subtle)]">
                    <div className="flex items-center justify-between gap-2">
                        <div className="flex items-center gap-2">
                            <div className="w-2 h-2 rounded-full bg-purple-400 animate-pulse" />
                            <span className="text-[10px] text-[var(--fg-secondary)] font-mono uppercase tracking-wide">
                                {pendingChanges.length} {pendingChanges.length === 1 ? 'change' : 'changes'} pending review
                            </span>
                        </div>
                        <div className="flex items-center gap-1.5">
                            <button
                                onClick={() => {
                                    // Reject all - just remove from pending list
                                    // The in-editor overlays will disappear automatically
                                    pendingChanges.forEach(change => rejectChange(change.id));
                                }}
                                className="flex items-center gap-1 px-2.5 py-1 rounded text-[11px] font-medium bg-red-600/80 hover:bg-red-500 text-white transition-colors"
                            >
                                <X className="w-3 h-3" />
                                {t('diff.rejectAll')}
                            </button>
                            <button
                                onClick={approveAllChanges}
                                className="flex items-center gap-1 px-2.5 py-1 rounded text-[11px] font-medium bg-emerald-600/80 hover:bg-emerald-500 text-white transition-colors"
                            >
                                <Check className="w-3 h-3" />
                                {t('diff.acceptAll')}
                            </button>
                        </div>
                    </div>
                </div>
            )}

            {/* Command Center */}
            <CommandCenter
                onSend={sendMessage}
                onStop={stopGeneration}
                loading={loading}
                models={models}
                selectedModelId={selectedModelId}
                setSelectedModelId={setSelectedModelId}
            />
        </div>
    );
};
