'use client';
import React, { useEffect, useRef } from 'react';
import { useTranslations } from 'next-intl';
import { Check, X } from 'lucide-react';
import { useChat } from '../hooks/useChat';
import { useCommandExecution } from '../hooks/useCommandExecution';
import { ChatMessage } from './ChatMessage';
import { ChatInput } from './ChatInput';
import { ModelSelector } from './ModelSelector';
import { ChatTerminal } from './ChatTerminal';

export const ChatPanel: React.FC = () => {
    const t = useTranslations();
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

    return (
        <div className="flex flex-col h-full bg-[#09090b] text-zinc-300 font-sans tracking-tight">
            {/* Header */}
            <header className="h-10 border-b border-zinc-800/50 flex items-center px-4 bg-zinc-950/80 backdrop-blur justify-between select-none shrink-0">
                <div className="font-medium text-zinc-400 text-sm tracking-tight flex items-center gap-2">
                    <span className="w-1.5 h-1.5 bg-emerald-500 rounded-full shadow-[0_0_8px_rgba(16,185,129,0.4)]"></span>
                    {t('app.name')}
                </div>
                <div className="text-[10px] uppercase tracking-wider text-zinc-600 font-mono flex items-center gap-3">
                    <ModelSelector
                        models={models}
                        selectedId={selectedModelId}
                        onSelect={setSelectedModelId}
                        disabled={loading}
                    />
                    <span>{messages.length} OPS</span>
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
                        <div className="flex flex-col items-center justify-center p-12 text-zinc-600 text-center space-y-4 select-none">
                            <div className="text-4xl opacity-20 filter grayscale">
                                🗡️
                            </div>
                            <h2 className="text-sm font-medium text-zinc-500 tracking-wide uppercase">System Ready</h2>
                            <p className="max-w-xs text-xs font-mono opacity-50">
                                Awaiting input. Surgical precision engaged.
                            </p>
                        </div>
                    )}

                    {messages.map((msg, idx) => {
                        // Show pending actions on the last assistant message
                        const isLastAssistant = idx === messages.length - 1 && msg.role === 'Assistant';
                        const showPendingActions = isLastAssistant && pendingActions && pendingActions.length > 0;
                        
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
                                />
                                
                                {/* Show terminals right after the message they belong to */}
                                {messageExecutions.map((exec) => exec && (
                                    <div key={exec.callId} className="px-4 py-2">
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
                            <div className="text-xs text-zinc-400">
                                {pendingChanges.map(c => c.path?.split('/').pop() || 'unknown').join(', ')}
                            </div>
                            <div className="text-xs text-zinc-500 mt-2">
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
                <div className="shrink-0 px-3 py-2 bg-zinc-900/30 border-t border-zinc-800/50">
                    <div className="flex items-center justify-between gap-2">
                        <div className="flex items-center gap-2">
                            <div className="w-2 h-2 rounded-full bg-purple-400 animate-pulse" />
                            <span className="text-[10px] text-zinc-400 font-mono uppercase tracking-wide">
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
