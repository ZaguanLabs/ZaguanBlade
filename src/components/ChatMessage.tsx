import React, { useState, useEffect, useRef, useCallback } from 'react';
import { ChatMessage as ChatMessageType } from '../types/chat';
import { User, Bot, Terminal, Brain, ChevronDown, ChevronRight, Loader2, Copy, RotateCcw, Pencil, MessageSquare } from 'lucide-react';
import { ToolCallDisplay } from './ToolCallDisplay';
import { ProgressIndicator } from './ProgressIndicator';
import { CommandOutputDisplay } from './CommandOutputDisplay';
import { CommandApprovalCard } from './CommandApprovalCard';
import { TodoList } from './TodoList';
import { useContextMenu, ContextMenuItem } from './ui/ContextMenu';
import { MarkdownRenderer } from './MarkdownRenderer';

const ReasoningBlock: React.FC<{ content: string; isActive?: boolean; hasContent?: boolean }> = ({ content, isActive, hasContent }) => {
    const [isExpanded, setIsExpanded] = useState(true); // Start expanded
    const [userToggled, setUserToggled] = useState(false); // Track if user manually toggled
    const contentRef = useRef<HTMLDivElement>(null);
    const wasActiveRef = useRef(isActive);
    const hadContentRef = useRef(hasContent);

    // Strip [THINKING] and [/THINKING] tags from content
    const cleanContent = content
        .replace(/\[THINKING\]/gi, '')
        .replace(/\[\/THINKING\]/gi, '')
        .trim();

    // Auto-expand when streaming starts, auto-collapse when content arrives
    useEffect(() => {
        // If message just became active and has no content yet, expand (unless user toggled)
        if (isActive && !wasActiveRef.current && !hasContent && !userToggled) {
            setIsExpanded(true);
        }

        // If content starts arriving (transition from no content to content), collapse
        // Only auto-collapse if user hasn't manually toggled
        if (hasContent && !hadContentRef.current && isExpanded && !userToggled) {
            setIsExpanded(false);
        }

        wasActiveRef.current = isActive;
        hadContentRef.current = hasContent;
    }, [isActive, hasContent, isExpanded, userToggled]);

    // Auto-scroll to bottom while streaming
    useEffect(() => {
        if (isExpanded && isActive && contentRef.current) {
            contentRef.current.scrollTop = contentRef.current.scrollHeight;
        }
    }, [cleanContent, isExpanded, isActive]);

    if (!cleanContent) return null;

    const handleToggle = () => {
        setIsExpanded(!isExpanded);
        setUserToggled(true); // Mark that user has manually controlled this
    };

    const isStreaming = isActive && !hasContent;

    return (
        <div className={`my-2 rounded-md border overflow-hidden transition-all duration-200 ${
            isStreaming 
                ? 'border-purple-500/30 bg-purple-950/10' 
                : 'border-zinc-800/50 bg-zinc-900/20'
        }`}>
            {/* Header - clickable to toggle */}
            <button
                onClick={handleToggle}
                className="w-full flex items-center gap-2 px-3 py-1.5 hover:bg-zinc-800/30 transition-colors text-left"
            >
                <div className="flex items-center gap-2">
                    <Brain className={`w-3 h-3 ${isStreaming ? 'text-purple-400 animate-pulse' : 'text-zinc-600'}`} />
                    <span className={`font-mono text-[9px] uppercase tracking-wider ${
                        isStreaming ? 'text-purple-400' : 'text-zinc-600'
                    }`}>
                        {isStreaming ? 'Thinking...' : 'Thought Process'}
                    </span>
                </div>
                <div className="flex-1" />
                {isStreaming && (
                    <Loader2 className="w-2.5 h-2.5 text-purple-400/60 animate-spin mr-1" />
                )}
                {isExpanded ? (
                    <ChevronDown className="w-3 h-3 text-zinc-600" />
                ) : (
                    <ChevronRight className="w-3 h-3 text-zinc-600" />
                )}
            </button>

            {/* Content - scrollable container */}
            {isExpanded && (
                <div 
                    ref={contentRef}
                    className="px-3 py-2 border-t border-zinc-800/30 bg-zinc-950/20 max-h-48 overflow-y-auto"
                >
                    <div className="text-zinc-500 text-[10px] leading-relaxed select-text whitespace-pre-wrap font-mono">
                        {cleanContent}
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

            <div className="flex-1 min-w-0 space-y-0.5 overflow-hidden">
                {!isContinued && (
                    <div className="flex items-center gap-2 h-4 mb-0.5">
                        <span className="font-semibold text-[10px] text-zinc-400">
                            {isUser ? 'User' : (isAssistant ? 'Assistant' : message.role)}
                        </span>
                        {isTool && message.tool_call_id && (
                            <span className="text-[10px] font-mono text-zinc-600 bg-zinc-900 border border-zinc-800 px-1.5 rounded-sm">
                                {message.tool_call_id.slice(0, 8)}
                            </span>
                        )}
                    </div>
                )}

                {/* Progress indicator from zcoderd */}
                {message.progress && (
                    <ProgressIndicator progress={message.progress} />
                )}

                {/* Thinking indicator for slow models - show when active but no content yet */}
                {isActive && isAssistant && !hasContent && !hasReasoning && !message.progress && (
                    <div className="flex items-center gap-2 py-2 text-zinc-500">
                        <Loader2 className="w-3.5 h-3.5 animate-spin text-emerald-500/70" />
                        <span className="text-[11px] font-mono">Thinking...</span>
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
                                    <div key={block.id || `text-${idx}`} className="mb-2 select-text">
                                        <MarkdownRenderer content={block.content} />
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

                        {message.todos && message.todos.length > 0 && <TodoList todos={message.todos} />}

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
                                    <div className="mb-2 select-text">
                                        <MarkdownRenderer content={initialText} />
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
                                    <div className="select-text">
                                        <MarkdownRenderer content={finalText} />
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
