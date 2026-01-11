import React, { useState } from 'react';
import ReactMarkdown from 'react-markdown';
import { ChatMessage as ChatMessageType } from '../types/chat';
import { User, Bot, Terminal, Brain, ChevronDown, ChevronRight } from 'lucide-react';
import { ToolCallDisplay } from './ToolCallDisplay';
import { ProgressIndicator } from './ProgressIndicator';
import { CommandOutputDisplay } from './CommandOutputDisplay';
import { CommandApprovalCard } from './CommandApprovalCard';
import { TodoList } from './TodoList';

const ReasoningBlock: React.FC<{ content: string }> = ({ content }) => {
    const [isExpanded, setIsExpanded] = useState(false);

    return (
        <div className="my-2 select-none">
            <button
                onClick={() => setIsExpanded(!isExpanded)}
                className="flex items-center gap-2 px-2 py-1 rounded hover:bg-zinc-800/50 transition-colors group/reasoning"
            >
                <div className="flex items-center gap-1.5 opacity-40 group-hover/reasoning:opacity-80 transition-opacity">
                    <Brain className="w-3 h-3 text-emerald-500" />
                    <span className="font-mono text-[9px] uppercase tracking-widest font-bold">Thoughts</span>
                </div>
                {isExpanded ? (
                    <ChevronDown className="w-3 h-3 opacity-20" />
                ) : (
                    <ChevronRight className="w-3 h-3 opacity-20" />
                )}
            </button>

            {isExpanded && (
                <div className="mt-1 ml-2 pl-3 border-l border-zinc-800/50 py-1 transition-all duration-300">
                    <div className="prose prose-invert prose-xs max-w-none text-zinc-500/70 font-mono text-[11px] leading-relaxed italic">
                        <ReactMarkdown>
                            {content}
                        </ReactMarkdown>
                    </div>
                </div>
            )}
        </div>
    );
};

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
                        {isUser ? 'User' : (isAssistant ? 'Assistant' : message.role)}
                    </span>
                    {isTool && message.tool_call_id && (
                        <span className="text-[10px] font-mono text-zinc-600 bg-zinc-900 border border-zinc-800 px-1.5 rounded-sm">
                            {message.tool_call_id.slice(0, 8)}
                        </span>
                    )}

                </div>

                {message.reasoning && (
                    <ReasoningBlock content={message.reasoning} />
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

                        // Fallback: If after_tools is empty/missing (e.g. strict OpenAI mode in backend),
                        // but main content has grown beyond initialText, infer the difference as finalText.
                        if (!finalText && message.content.length > initialText.length && message.content.startsWith(initialText)) {
                            finalText = message.content.slice(initialText.length);
                        }
                    } else {
                        // Default behavior: All content is considered "Pre-Tool" text (Reasoning/Explanation)
                        // This ensures it renders BEFORE the tool calls, which matches the standard flow:
                        // "I will do X" -> [Tool Call]
                        initialText = message.content;
                        finalText = '';
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
