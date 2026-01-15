import React, { useState, useEffect, useRef, useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import { ChatMessage as ChatMessageType } from '../types/chat';
import { User, Bot, Terminal, Brain, ChevronDown, ChevronRight, Loader2, Copy, RotateCcw, Pencil, MessageSquare } from 'lucide-react';
import { ToolCallDisplay } from './ToolCallDisplay';
import { ProgressIndicator } from './ProgressIndicator';
import { CommandOutputDisplay } from './CommandOutputDisplay';
import { CommandApprovalCard } from './CommandApprovalCard';
import { TodoList } from './TodoList';
import { useContextMenu, ContextMenuItem } from './ui/ContextMenu';

const ReasoningBlock: React.FC<{ content: string; isActive?: boolean; hasContent?: boolean }> = ({ content, isActive, hasContent }) => {
    const [isExpanded, setIsExpanded] = useState(false);
    const wasActiveRef = useRef(isActive);

    // Auto-expand/collapse logic
    useEffect(() => {
        // If message just became active (started streaming) and has no content yet, expand reasoning
        if (isActive && !wasActiveRef.current && !hasContent) {
            setIsExpanded(true);
        }

        // If content starts arriving, collapse reasoning
        if (hasContent && isExpanded && isActive) {
            setIsExpanded(false);
        }

        wasActiveRef.current = isActive;
    }, [isActive, hasContent, isExpanded]);

    if (!content) return null;

    return (
        <div className="my-3 rounded-md border border-zinc-800 bg-zinc-900/40 overflow-hidden group/reasoning">
            <button
                onClick={() => setIsExpanded(!isExpanded)}
                className="w-full flex items-center gap-2 px-3 py-2 hover:bg-zinc-800/50 transition-colors text-left"
            >
                <div className="flex items-center gap-2 text-zinc-500 group-hover/reasoning:text-zinc-400 transition-colors">
                    <Brain className={`w-3.5 h-3.5 ${isActive && !hasContent ? 'text-purple-400 animate-pulse' : ''}`} />
                    <span className="font-mono text-[10px] uppercase tracking-wider font-semibold">
                        {isActive && !hasContent ? 'Reasoning...' : 'Reasoning Process'}
                    </span>
                </div>
                <div className="flex-1" />
                {isActive && !hasContent && (
                    <Loader2 className="w-3 h-3 text-purple-500/50 animate-spin mr-2" />
                )}
                {isExpanded ? (
                    <ChevronDown className="w-3.5 h-3.5 text-zinc-600" />
                ) : (
                    <ChevronRight className="w-3.5 h-3.5 text-zinc-600" />
                )}
            </button>

            {isExpanded && (
                <div className="px-3 py-3 border-t border-zinc-800/50 bg-zinc-950/30">
                    <div className="prose prose-invert prose-xs max-w-none text-zinc-400 font-mono text-[11px] leading-relaxed select-text">
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
    isContinued?: boolean; // For visual grouping
    isActive?: boolean; // Is this the currently streaming message?
}

export const ChatMessage: React.FC<ChatMessageProps> = ({
    message,
    pendingActions,
    onApproveCommand,
    onSkipCommand,
    isContinued = false,
    isActive = false
}) => {
    const isUser = message.role === 'User';
    const isSystem = message.role === 'System';
    const isTool = message.role === 'Tool';
    const isAssistant = message.role === 'Assistant';
    const { showMenu } = useContextMenu();

    // Don't render Tool messages separately - they're shown in the tool call display
    // UNLESS this is a standalone tool message not handled by the previous assistant message.
    // However, for the "Single Turn" view, we rely on the Assistant message containing the calls.
    if (isTool) {
        return null;
    }

    const hasReasoning = !!message.reasoning;

    // Determine content split for rendering tool calls in the middle
    const toolCalls = (message.tool_calls || []).filter(
        (call) => call.function.name !== 'todo_write'
    );
    const hasToolCalls = toolCalls.length > 0;

    // Use explicit fields from protocol if available
    const hasExplicitSplit = message.content_before_tools !== undefined || message.content_after_tools !== undefined;
    let initialText = '';
    let finalText = '';

    if (hasExplicitSplit) {
        initialText = message.content_before_tools || '';
        finalText = message.content_after_tools || '';
        // Fallback inference if final text missing but content grew
        if (!finalText && message.content.length > initialText.length && message.content.startsWith(initialText)) {
            finalText = message.content.slice(initialText.length);
        }
    } else {
        // Default: Content is pre-tool
        initialText = message.content;
        finalText = '';
    }

    const hasContent = initialText.length > 0 || finalText.length > 0;

    // Context menu for chat messages
    const handleContextMenu = useCallback((e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();

        const items: ContextMenuItem[] = [
            {
                id: 'copy-message',
                label: 'Copy Message',
                icon: <Copy className="w-4 h-4" />,
                shortcut: 'Ctrl+C',
                onClick: async () => {
                    try {
                        await navigator.clipboard.writeText(message.content);
                        console.log('[Context] Copied message');
                    } catch (err) {
                        console.error('[Context] Failed to copy:', err);
                    }
                }
            },
            {
                id: 'copy-markdown',
                label: 'Copy as Markdown',
                icon: <MessageSquare className="w-4 h-4" />,
                onClick: async () => {
                    try {
                        const markdown = `**${message.role}:**\n\n${message.content}`;
                        await navigator.clipboard.writeText(markdown);
                        console.log('[Context] Copied as markdown');
                    } catch (err) {
                        console.error('[Context] Failed to copy:', err);
                    }
                }
            },
        ];

        if (isUser) {
            items.push(
                { id: 'div-1', label: '', divider: true },
                {
                    id: 'edit-message',
                    label: 'Edit Message',
                    icon: <Pencil className="w-4 h-4" />,
                    onClick: () => {
                        // TODO: Implement edit message functionality
                        console.log('[Context] Edit message');
                    }
                }
            );
        }

        if (isAssistant) {
            items.push(
                { id: 'div-1', label: '', divider: true },
                {
                    id: 'regenerate',
                    label: 'Regenerate Response',
                    icon: <RotateCcw className="w-4 h-4" />,
                    onClick: () => {
                        // TODO: Implement regenerate
                        console.log('[Context] Regenerate response');
                    }
                }
            );
        }

        showMenu({ x: e.clientX, y: e.clientY }, items);
    }, [message, isUser, isAssistant, showMenu]);

    return (
        <div
            className={`flex gap-3 px-4 group ${isContinued ? 'pt-1 pb-2' : 'pt-4 pb-4'} ${isUser ? 'bg-zinc-800/10' : ''} ${isTool ? 'opacity-70' : ''}`}
            onContextMenu={handleContextMenu}
        >
            {/* Avatar Column */}
            <div className="w-5 shrink-0 flex flex-col items-center">
                {!isContinued && (
                    <div className="mt-0.5 opacity-80 group-hover:opacity-100 transition-opacity">
                        {isUser && <div className="w-5 h-5 rounded-full bg-zinc-700/50 flex items-center justify-center"><User className="w-3 h-3 text-zinc-300" /></div>}
                        {isAssistant && <div className="w-5 h-5 rounded-full bg-emerald-500/10 flex items-center justify-center"><Bot className="w-3.5 h-3.5 text-emerald-500" /></div>}
                        {isSystem && <Terminal className="w-4 h-4 text-yellow-600" />}
                        {isTool && <Terminal className="w-4 h-4 text-purple-500/80" />}
                    </div>
                )}
                {/* Visual connector line for continued messages */}
                {isContinued && (
                    <div className="w-px h-full bg-zinc-800/50" />
                )}
            </div>

            <div className="flex-1 min-w-0 space-y-1 overflow-hidden">
                {!isContinued && (
                    <div className="flex items-center gap-2 h-5 mb-1">
                        <span className="font-semibold text-[11px] text-zinc-400">
                            {isUser ? 'User' : (isAssistant ? 'Assistant' : message.role)}
                        </span>
                        {isTool && message.tool_call_id && (
                            <span className="text-[10px] font-mono text-zinc-600 bg-zinc-900 border border-zinc-800 px-1.5 rounded-sm">
                                {message.tool_call_id.slice(0, 8)}
                            </span>
                        )}
                    </div>
                )}

                {/* Render Interleaved Blocks if available, else Legacy Fallback */}
                {message.blocks && message.blocks.length > 0 ? (
                    <>
                        {message.blocks.map((block, idx) => {
                            if (block.type === 'reasoning') {
                                return (
                                    <ReasoningBlock
                                        key={block.id || `reasoning-${idx}`}
                                        content={block.content}
                                        isActive={isActive && idx === message.blocks!.length - 1}
                                        hasContent={false} // Interleaved reasoning acts somewhat independent
                                    />
                                );
                            } else if (block.type === 'text') {
                                return (
                                    <div key={block.id || `text-${idx}`} className="prose prose-invert prose-xs max-w-none text-zinc-300 leading-relaxed mb-3 select-text blocks-text
                                        prose-headings:text-zinc-200 prose-headings:font-semibold
                                        prose-p:text-zinc-300 prose-p:leading-relaxed prose-p:my-2
                                        prose-strong:text-zinc-100 prose-strong:font-semibold
                                        prose-a:text-emerald-400 prose-a:no-underline hover:prose-a:underline
                                        prose-code:text-emerald-400 prose-code:bg-zinc-900 prose-code:px-1 prose-code:py-0.5 prose-code:rounded
                                        prose-pre:bg-zinc-900 prose-pre:border prose-pre:border-zinc-800
                                        prose-ul:text-zinc-300 prose-ul:my-2 prose-ol:text-zinc-300 prose-ol:my-2
                                        prose-li:text-zinc-300 prose-li:leading-relaxed prose-li:my-1
                                        prose-blockquote:border-l-emerald-500 prose-blockquote:text-zinc-400">
                                        <ReactMarkdown>{block.content}</ReactMarkdown>
                                    </div>
                                );
                            } else if (block.type === 'tool_call') {
                                const toolCall = message.tool_calls?.find(tc => tc.id === block.id);
                                if (!toolCall) return null;
                                return (
                                    <div key={block.id} className="mb-3 space-y-2">
                                        <ToolCallDisplay
                                            toolCall={toolCall}
                                            status={toolCall.status || 'executing'}
                                            result={toolCall.result}
                                        />
                                    </div>
                                );
                            }
                            return null;
                        })}

                        {/* Pending Actions (Command Approval) */}
                        {pendingActions && pendingActions.length > 0 && onApproveCommand && onSkipCommand && (
                            <div className="mb-3">
                                <CommandApprovalCard
                                    actions={pendingActions}
                                    onRun={onApproveCommand}
                                    onSkip={onSkipCommand}
                                />
                            </div>
                        )}

                        {/* Command Output/Executions are typically appended at end */}
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
                ) : (
                    // Legacy Rendering Fallback (Pre-Blocks)
                    (() => {
                        const toolCalls = (message.tool_calls || []).filter(
                            (call) => call.function.name !== 'todo_write'
                        );
                        // ... (keep existing legacy logic if needed)
                        // Actually, since we rewrite the component content, I will just paste the logic below
                        const hasToolCalls = toolCalls.length > 0;
                        const hasExplicitSplit = message.content_before_tools !== undefined || message.content_after_tools !== undefined;
                        let initialText = '';
                        let finalText = '';

                        if (hasExplicitSplit) {
                            initialText = message.content_before_tools || '';
                            finalText = message.content_after_tools || '';
                            if (!finalText && message.content.length > initialText.length && message.content.startsWith(initialText)) {
                                finalText = message.content.slice(initialText.length);
                            }
                        } else {
                            initialText = message.content;
                        }

                        return (
                            <>
                                {hasReasoning && (
                                    <ReasoningBlock
                                        content={message.reasoning!}
                                        isActive={isActive}
                                        hasContent={hasContent}
                                    />
                                )}
                                {initialText && (
                                    <div className="prose prose-invert prose-xs max-w-none text-zinc-300 leading-relaxed mb-3 select-text">
                                        <ReactMarkdown>{initialText}</ReactMarkdown>
                                    </div>
                                )}
                                {pendingActions && pendingActions.length > 0 && onApproveCommand && onSkipCommand && (
                                    <div className="mb-3">
                                        <CommandApprovalCard
                                            actions={pendingActions}
                                            onRun={onApproveCommand}
                                            onSkip={onSkipCommand}
                                        />
                                    </div>
                                )}
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
                                {message.todos && message.todos.length > 0 && <TodoList todos={message.todos} />}
                                {finalText && (
                                    <div className="prose prose-invert prose-xs max-w-none text-zinc-300 leading-relaxed select-text">
                                        <ReactMarkdown>{finalText}</ReactMarkdown>
                                    </div>
                                )}
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
                    })()
                )}
            </div>
        </div>
    );
};
