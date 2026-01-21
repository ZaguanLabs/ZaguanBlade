import React, { useEffect, useRef, useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { Check, X, Settings, Key } from 'lucide-react';
import { useCommandExecution } from '../hooks/useCommandExecution';
import { useHistory } from '../hooks/useHistory';
import type { ChatMessage as ChatMessageType, ModelInfo } from '../types/chat';

import type { StructuredAction } from '../types/events';
import type { ApiConfig } from '../types/settings';
import { ChatMessage } from './ChatMessage';
import { ChatTabBar } from './ChatTabBar';
import { CommandCenter } from './CommandCenter';
import { HistoryTab } from './HistoryTab';
import { ChatTerminal } from './ChatTerminal';
import { ProgressIndicator } from './ProgressIndicator';

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
    const [hasApiKey, setHasApiKey] = useState<boolean>(true);

    // Check API Key
    const checkApiKey = useCallback(async () => {
        try {
            const config = await invoke<ApiConfig>('get_global_settings');
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

    // Auto-scroll logic
    useEffect(() => {
        // If we just sent a message (loading became true and it's a new user message), force scroll
        // Or if we are already at bottom, keep scrolling.

        // Check if the last message is User, implies we just sent it -> Force Scroll
        const lastMsg = messages[messages.length - 1];
        const justSent = lastMsg?.role === 'User';

        if (justSent || isUserAtBottomRef.current) {
            // When loading (streaming), avoid smooth scroll as it can lag behind rapid updates
            messagesEndRef.current?.scrollIntoView({
                behavior: loading ? 'auto' : 'smooth'
            });
        }
    }, [messages, loading]);

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
            const conversationMessages = await loadConversation(sessionId);
            onLoadConversation(conversationMessages);
            setActiveTab('chat');
        } catch (e) {
            console.error('Failed to load conversation:', e);
        }
    }, [loadConversation, onLoadConversation]);

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
                        // Use a larger threshold (100px) to be more resilient to large appends (e.g. code blocks)
                        const isBottom = Math.abs(target.scrollHeight - target.scrollTop - target.clientHeight) < 100;
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



                            return (
                                <React.Fragment key={idx}>
                                    <ChatMessage
                                        message={msg}
                                        pendingActions={showPendingActions ? pendingActions : undefined}
                                        onApproveCommand={showPendingActions ? () => approveToolDecision('approve_once') : undefined}
                                        onSkipCommand={showPendingActions ? () => approveToolDecision('reject') : undefined}
                                        isContinued={isContinued}
                                        isActive={isActive}
                                        activeTerminals={executions}
                                        onTerminalComplete={handleCommandComplete}
                                    />


                                </React.Fragment>
                            );
                        })}

                        {/* Research progress indicator */}
                        {researchProgress?.isActive && (
                            <div className="px-4">
                                <ProgressIndicator progress={researchProgress} />
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
                    projectId={projectId}
                    onSelectConversation={handleSelectConversation}
                />
            )}



            <CommandCenter
                onSend={sendMessage}
                onStop={stopGeneration}
                loading={loading}
                models={models}
                selectedModelId={selectedModelId}
                setSelectedModelId={setSelectedModelId}
                disabled={!hasApiKey}
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
