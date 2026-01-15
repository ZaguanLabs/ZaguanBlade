import React, { useEffect, useRef, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, X } from 'lucide-react';
import { useChat } from '../hooks/useChat';
import { useCommandExecution } from '../hooks/useCommandExecution';
import { ChatMessage } from './ChatMessage';
import { ChatInput } from './ChatInput';
import { ModelSelector } from './ModelSelector';
import { ChatTerminal } from './ChatTerminal';

export const ChatPanel: React.FC = () => {
    const { t } = useTranslation();
    const {
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
    } = useChat();
    const { executions, handleCommandComplete } = useCommandExecution();
    const messagesEndRef = useRef<HTMLDivElement>(null);
    const isUserAtBottomRef = useRef(true);

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

    return (
        <div className="flex flex-col h-full bg-[var(--bg-app)] text-[var(--fg-primary)] font-sans tracking-tight" onContextMenu={handleContextMenu}>
            {/* Header */}
            {/* Header */}
            <header className="h-10 border-b border-[var(--border-subtle)] flex items-center px-2 bg-[var(--bg-app)] select-none shrink-0 z-30">
                <div className="w-full">
                    <ModelSelector
                        models={models}
                        selectedId={selectedModelId || ''}
                        onSelect={setSelectedModelId}
                    />
                </div>
            </header>

            {/* Messages */}
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

            {/* Input */}
            <div className="shrink-0 z-20">
                <ChatInput onSend={sendMessage} onStop={stopGeneration} loading={loading} />
            </div>

        </div>
    );
};
