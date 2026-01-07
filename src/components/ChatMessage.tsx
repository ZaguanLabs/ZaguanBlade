'use client';
import React from 'react';
import ReactMarkdown from 'react-markdown';
import { ChatMessage as ChatMessageType } from '../types/chat';
import { User, Bot, Terminal } from 'lucide-react';
import { ToolCallDisplay } from './ToolCallDisplay';
import { ProgressIndicator } from './ProgressIndicator';
import { CommandOutputDisplay } from './CommandOutputDisplay';
import { CommandApprovalCard } from './CommandApprovalCard';
import { TodoList } from './TodoList';

interface ChatMessageProps {
    message: ChatMessageType;
    pendingActions?: import('../types/events').StructuredAction[];
    onApproveCommand?: () => void;
    onSkipCommand?: () => void;
}

export const ChatMessage: React.FC<ChatMessageProps> = ({ message, pendingActions, onApproveCommand, onSkipCommand }) => {
    const isUser = message.role === 'User';
    const isSystem = message.role === 'System';
    const isTool = message.role === 'Tool';
    const isAssistant = message.role === 'Assistant';
    
    // Don't render Tool messages separately - they're shown in the tool call display
    if (isTool) {
        return null;
    }

    return (
        <div className={`flex gap-3 px-4 py-3 group ${isUser ? 'bg-zinc-800/10' : ''} ${isTool ? 'opacity-70' : ''}`}>
            <div className="shrink-0 mt-0.5 opacity-50 group-hover:opacity-100 transition-opacity">
                {isUser && <User className="w-4 h-4 text-zinc-500" />}
                {isAssistant && <Bot className="w-4 h-4 text-emerald-500/80" />}
                {isSystem && <Terminal className="w-4 h-4 text-yellow-600" />}
                {isTool && <Terminal className="w-4 h-4 text-purple-500/80" />}
            </div>

            <div className="flex-1 min-w-0 space-y-1 overflow-hidden">
                <div className="flex items-center gap-2 h-5">
                    <span className="font-medium text-xs text-zinc-500 uppercase tracking-wider">
                        {isUser ? 'Operator' : (isAssistant ? 'System' : message.role)}
                    </span>
                    {isTool && message.tool_call_id && (
                        <span className="text-[10px] font-mono text-zinc-600 bg-zinc-900 border border-zinc-800 px-1.5 rounded-sm">
                            {message.tool_call_id.slice(0, 8)}
                        </span>
                    )}
                    {/* DEBUG: Show data state */}
                    {isAssistant && (
                        <span className="text-[9px] font-mono text-yellow-600 bg-yellow-950/20 border border-yellow-800/30 px-1.5 py-0.5 rounded">
                            content:{message.content.length} tools:{message.tool_calls?.length || 0}
                        </span>
                    )}
                </div>

                {message.reasoning && (
                    <div className="bg-zinc-950/30 border-l-2 border-zinc-800 pl-3 py-1 my-2">
                        <div className="font-mono text-[10px] text-zinc-600 uppercase mb-0.5 flex items-center gap-1">
                            <span>Process Logic</span>
                        </div>
                        <div className="prose prose-invert prose-xs max-w-none text-zinc-500/80 font-mono text-[11px] leading-relaxed">
                            {message.reasoning}
                        </div>
                    </div>
                )}

                {message.progress && (
                    <ProgressIndicator progress={message.progress} />
                )}

                {/* Render in correct order: Initial text → Tool calls → Final text */}
                {(() => {
                    const toolCalls = (message.tool_calls || []).filter(
                        (call) => call.function.name !== 'todo_write'
                    );
                    const hasToolCalls = toolCalls.length > 0;
                    
                    // Use explicit fields from protocol if available
                    // If content_before_tools and content_after_tools are both empty/missing,
                    // treat all content as final text (after tools)
                    const hasExplicitSplit = message.content_before_tools !== undefined || message.content_after_tools !== undefined;
                    
                    let initialText = '';
                    let finalText = '';
                    
                    if (hasExplicitSplit) {
                        // Protocol provided explicit split
                        initialText = message.content_before_tools || '';
                        finalText = message.content_after_tools || '';
                    } else if (hasToolCalls) {
                        // Has tool calls but no explicit split - all content is final
                        finalText = message.content;
                    } else {
                        // No tool calls - all content is final
                        finalText = message.content;
                    }
                    
                    return (
                        <>
                            {/* 1. INITIAL TEXT - before tool execution */}
                            {initialText && (
                                <div className="prose prose-invert prose-sm max-w-none text-zinc-300 leading-relaxed mb-3
                                    prose-headings:text-zinc-200 prose-headings:font-semibold
                                    prose-p:text-zinc-300 prose-p:leading-relaxed prose-p:my-2
                                    prose-strong:text-zinc-100 prose-strong:font-semibold
                                    prose-a:text-emerald-400 prose-a:no-underline hover:prose-a:underline
                                    prose-code:text-emerald-400 prose-code:bg-zinc-900 prose-code:px-1 prose-code:py-0.5 prose-code:rounded
                                    prose-pre:bg-zinc-900 prose-pre:border prose-pre:border-zinc-800
                                    prose-ul:text-zinc-300 prose-ul:my-2 prose-ol:text-zinc-300 prose-ol:my-2
                                    prose-li:text-zinc-300 prose-li:leading-relaxed prose-li:my-1
                                    prose-blockquote:border-l-emerald-500 prose-blockquote:text-zinc-400">
                                    <ReactMarkdown>
                                        {initialText}
                                    </ReactMarkdown>
                                </div>
                            )}

                            {/* 2. COMMAND APPROVAL - if pending */}
                            {pendingActions && pendingActions.length > 0 && onApproveCommand && onSkipCommand && (
                                <div className="mb-3">
                                    <CommandApprovalCard
                                        actions={pendingActions}
                                        onRun={onApproveCommand}
                                        onSkip={onSkipCommand}
                                    />
                                </div>
                            )}

                            {/* 3. TOOL CALLS - in the middle */}
                            {hasToolCalls && (
                                <div className="mb-3 space-y-2">
                                    {toolCalls.map((call, idx) => (
                                        <ToolCallDisplay 
                                            key={`${call.id}-${idx}`}
                                            toolCall={call}
                                            status={call.status || 'executing'}
                                            result={call.result}
                                        />
                                    ))}
                                </div>
                            )}

                            {/* TODO LIST - task progress tracking */}
                            {message.todos && message.todos.length > 0 && (
                                <TodoList todos={message.todos} />
                            )}

                            {/* FINAL TEXT - after tool execution */}
                            {finalText && (
                                <div className="prose prose-invert prose-sm max-w-none text-zinc-300 leading-relaxed
                                    prose-headings:text-zinc-200 prose-headings:font-semibold
                                    prose-p:text-zinc-300 prose-p:leading-relaxed prose-p:my-2
                                    prose-strong:text-zinc-100 prose-strong:font-semibold
                                    prose-a:text-emerald-400 prose-a:no-underline hover:prose-a:underline
                                    prose-code:text-emerald-400 prose-code:bg-zinc-900 prose-code:px-1 prose-code:py-0.5 prose-code:rounded
                                    prose-pre:bg-zinc-900 prose-pre:border prose-pre:border-zinc-800
                                    prose-ul:text-zinc-300 prose-ul:my-2 prose-ol:text-zinc-300 prose-ol:my-2
                                    prose-li:text-zinc-300 prose-li:leading-relaxed prose-li:my-1
                                    prose-blockquote:border-l-emerald-500 prose-blockquote:text-zinc-400">
                                    <ReactMarkdown>
                                        {finalText}
                                    </ReactMarkdown>
                                </div>
                            )}
                            
                            {/* Command Executions */}
                            {message.commandExecutions && message.commandExecutions.length > 0 && (
                                <div className="mt-3 space-y-2">
                                    {message.commandExecutions.map((cmd, idx) => (
                                        <CommandOutputDisplay
                                            key={`${cmd.timestamp}-${idx}`}
                                            command={cmd.command}
                                            cwd={cmd.cwd}
                                            output={cmd.output}
                                            exitCode={cmd.exitCode}
                                            duration={cmd.duration}
                                        />
                                    ))}
                                </div>
                            )}
                        </>
                    );
                })()}
            </div>
        </div>
    );
};
