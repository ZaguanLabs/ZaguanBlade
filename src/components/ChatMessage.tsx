import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import type { ChatMessage as ChatMessageType, ChatImage, ImageAttachment, ToolCall, CommandExecution, MessageBlock } from '../types/chat';
import { User, Bot, Terminal, Brain, ChevronDown, ChevronRight, Loader2, Copy, RotateCcw, Pencil, MessageSquare, Check, FileText, Folder } from 'lucide-react';
import { ToolCallDisplay } from './ToolCallDisplay';
import { CommandOutputDisplay } from './CommandOutputDisplay';
import { CommandApprovalCard } from './CommandApprovalCard';
import { useContextMenu, ContextMenuItem } from './ui/ContextMenu';
import { MarkdownRenderer } from './MarkdownRenderer';

const REVERTIBLE_TOOLS = new Set([
    'apply_patch',
    'edit_file',
    'write_file',
    'create_file',
    'delete_file',
    'replace_file_content',
    'multi_replace_file_content',
    'write_to_file',
]);

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

    const displayContent = cleanContent || content;

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
    }, [displayContent, isExpanded, isActive]);

    // DEBUG: Show raw content even if empty after cleaning, to see what's being suppressed
    if (!displayContent) return null;

    const handleToggle = () => {
        setIsExpanded(!isExpanded);
        setUserToggled(true); // Mark that user has manually controlled this
    };

    const isStreaming = isActive && !hasContent;

    return (
        <div className={`my-2 rounded-md border overflow-hidden transition-all duration-200 ${
            isStreaming
                ? 'border-purple-500/40 bg-purple-950/30 shadow-lg shadow-purple-500/10'
                : 'border-zinc-700/30 bg-zinc-800/20 opacity-70'
            }`}>
            {/* Header - clickable to toggle */}
            <button
                onClick={handleToggle}
                className="w-full flex items-center gap-2 px-3 py-1.5 hover:bg-zinc-700/30 transition-colors text-left"
            >
                <div className="flex items-center gap-2 flex-1 min-w-0">
                    <Brain className={`w-3 h-3 flex-shrink-0 ${isStreaming ? 'text-purple-400 animate-pulse' : 'text-zinc-600'}`} />
                    <span className={`font-mono text-[9px] uppercase tracking-wider flex-shrink-0 ${isStreaming ? 'text-purple-400' : 'text-zinc-600'
                        }`}>
                        {isStreaming ? 'Reasoning' : 'Thought Process'}
                    </span>
                    {!isExpanded && displayContent && (
                        <span className="text-[10px] text-zinc-500 truncate font-mono ml-2">
                            {displayContent.slice(0, 80)}...
                        </span>
                    )}
                </div>
                {isStreaming && (
                    <Loader2 className="w-2.5 h-2.5 text-purple-400/60 animate-spin mr-1 flex-shrink-0" />
                )}
                {isExpanded ? (
                    <ChevronDown className="w-3 h-3 text-zinc-500 flex-shrink-0" />
                ) : (
                    <ChevronRight className="w-3 h-3 text-zinc-500 flex-shrink-0" />
                )}
            </button>

            {/* Content - scrollable container */}
            {isExpanded && (
                <div
                    ref={contentRef}
                    className="px-3 py-2 border-t border-zinc-700/30 bg-zinc-800/40 max-h-48 overflow-y-auto overflow-x-hidden"
                >
                    <div className="text-zinc-400 text-[10px] leading-relaxed select-text whitespace-pre-wrap font-mono break-words overflow-wrap-anywhere">
                        {displayContent}
                    </div>
                </div>
            )}
        </div>
    );
};

const resolveImageUrls = (image: ChatImage) => {
    const attachment = image as ImageAttachment;
    const fullUrl = attachment.dataUrl
        || (image.data && image.mime_type ? `data:${image.mime_type};base64,${image.data}` : '');
    // Render high-resolution previews in chat; thumbnailUrl is only for compact attachment chips.
    const previewUrl = fullUrl || attachment.thumbnailUrl;
    return {
        fullUrl,
        previewUrl,
        name: image.name,
    };
};

const PlanSummaryDisplay: React.FC<{ todos: import('../types/events').TodoItem[] }> = ({ todos }) => {
    const [isExpanded, setIsExpanded] = useState(false);
    const completedCount = todos.filter(t => t.status === 'completed').length;

    return (
        <div className="my-2">
            <button
                onClick={() => setIsExpanded(!isExpanded)}
                className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-emerald-500/5 border border-emerald-500/20 hover:bg-emerald-500/10 transition-colors text-left w-full"
            >
                <Check className="w-3.5 h-3.5 text-emerald-400 flex-shrink-0" />
                <span className="text-[11px] text-emerald-400 font-medium">
                    Plan completed ({completedCount}/{todos.length} tasks)
                </span>
                {isExpanded ? (
                    <ChevronDown className="w-3 h-3 text-zinc-500 ml-auto flex-shrink-0" />
                ) : (
                    <ChevronRight className="w-3 h-3 text-zinc-500 ml-auto flex-shrink-0" />
                )}
            </button>
            {isExpanded && (
                <div className="mt-1 px-3 py-2 rounded-md bg-emerald-500/5 border border-emerald-500/10 space-y-0.5">
                    {todos.map((todo, idx) => (
                        <div key={idx} className="flex items-center gap-2 text-[11px]">
                            <Check className="w-3 h-3 text-emerald-400/60 flex-shrink-0" />
                            <span className="text-zinc-500 line-through">{todo.content}</span>
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
};

const ReferencedPathsDisplay: React.FC<{
    mentions: NonNullable<ChatMessageType['mentions']>;
    onOpenFile?: (path: string) => void;
}> = ({ mentions, onOpenFile }) => {
    if (mentions.length === 0) {
        return null;
    }

    return (
        <div className="mb-2 overflow-hidden rounded-lg border border-emerald-500/15 bg-emerald-500/6">
            <div className="flex items-center gap-2 border-b border-emerald-500/10 px-3 py-1.5 text-[10px] text-emerald-300/90">
                <span className="font-semibold uppercase tracking-[0.16em]">Referenced paths</span>
                <span className="text-emerald-400/60">{mentions.length}</span>
            </div>
            <div className="flex flex-wrap gap-1.5 px-3 py-2">
                {mentions.map((mention) => {
                    const isClickable = !mention.is_dir && !!onOpenFile;
                    const content = (
                        <>
                            {mention.is_dir ? (
                                <Folder className="h-3 w-3 shrink-0 text-emerald-300/80" />
                            ) : (
                                <FileText className="h-3 w-3 shrink-0 text-emerald-300/80" />
                            )}
                            <span className="truncate">{mention.path}</span>
                        </>
                    );

                    if (!isClickable) {
                        return (
                            <div
                                key={`${mention.kind}:${mention.path}`}
                                className="inline-flex max-w-full items-center gap-1.5 rounded-md border border-emerald-500/15 bg-zinc-950/30 px-2 py-1 text-[10px] text-zinc-300"
                                title={mention.path}
                            >
                                {content}
                            </div>
                        );
                    }

                    return (
                        <button
                            key={`${mention.kind}:${mention.path}`}
                            type="button"
                            onClick={() => onOpenFile?.(mention.path)}
                            className="inline-flex max-w-full items-center gap-1.5 rounded-md border border-emerald-500/15 bg-zinc-950/30 px-2 py-1 text-[10px] text-zinc-300 transition-colors hover:bg-emerald-500/10 hover:text-emerald-200"
                            title={`Open ${mention.path}`}
                        >
                            {content}
                        </button>
                    );
                })}
            </div>
        </div>
    );
};

type ActivityGroupItem =
    | { kind: 'tool_call'; id: string; toolCall: ToolCall }
    | { kind: 'command_execution'; id: string; commandExecution: CommandExecution };

type RenderSegment =
    | { kind: 'block'; block: MessageBlock; index: number }
    | { kind: 'activity_group'; id: string; items: ActivityGroupItem[] };

const ActivityGroupDisplay: React.FC<{
    items: ActivityGroupItem[];
    pendingActions?: import('../types/events').StructuredAction[];
    onUndoTool?: (toolCallId: string) => void;
    onStopCommand?: (callId: string) => void;
    onOpenFile?: (path: string) => void;
}> = ({ items, pendingActions, onUndoTool, onStopCommand, onOpenFile }) => {
    const [isExpanded, setIsExpanded] = useState(false);
    const hiddenCount = Math.max(0, items.length - 1);
    const summaryLabel = items.length === 1 ? 'Activity' : `Activity (${items.length})`;

    return (
        <div className="mb-3 overflow-hidden rounded-lg border border-zinc-800/70 bg-zinc-950/35">
            <button
                type="button"
                onClick={() => setIsExpanded((prev) => !prev)}
                className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-zinc-900/35"
            >
                <span className="text-[10px] font-semibold uppercase tracking-[0.14em] text-zinc-400">
                    {summaryLabel}
                </span>
                {hiddenCount > 0 && !isExpanded && (
                    <span className="text-[10px] text-zinc-500">
                        {hiddenCount} more
                    </span>
                )}
                <span className="ml-auto text-zinc-500">
                    {isExpanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
                </span>
            </button>
            <div className={isExpanded ? 'border-t border-zinc-800/60 px-2.5 py-2.5 space-y-2' : 'border-t border-zinc-800/60 px-2.5 py-2.5'}>
                {(isExpanded ? items : items.slice(0, 1)).map((item) => {
                    if (item.kind === 'tool_call') {
                        const toolCall = item.toolCall;
                        return (
                            <ToolCallDisplay
                                key={item.id}
                                toolCall={toolCall}
                                status={toolCall.status || 'executing'}
                                result={toolCall.result}
                                onStopCommand={
                                    toolCall.function.name === 'run_command' && onStopCommand
                                        ? (() => onStopCommand(toolCall.id))
                                        : undefined
                                }
                                onUndo={
                                    onUndoTool && REVERTIBLE_TOOLS.has(toolCall.function.name)
                                        ? (() => onUndoTool(toolCall.id))
                                        : undefined
                                }
                                onOpenFile={onOpenFile}
                            />
                        );
                    }

                    return (
                        <CommandOutputDisplay
                            key={item.id}
                            command={item.commandExecution.command}
                            cwd={item.commandExecution.cwd}
                            output={item.commandExecution.output}
                            exitCode={item.commandExecution.exitCode}
                            duration={item.commandExecution.duration}
                        />
                    );
                })}
            </div>
        </div>
    );
};

interface ChatMessageProps {
    message: ChatMessageType;
    pendingActions?: import('../types/events').StructuredAction[];
    onApproveCommand?: () => void;
    onSkipCommand?: () => void;
    onApproveSingleCommand?: (callId: string) => void;
    onSkipSingleCommand?: (callId: string) => void;
    isContinued?: boolean; // For visual grouping
    isActive?: boolean; // Is this the currently streaming message?
    onUndoTool?: (toolCallId: string) => void;
    onStopCommand?: (callId: string) => void;
    onOpenFile?: (path: string) => void;
}

const StreamingTextPreview: React.FC<{ content: string }> = ({ content }) => (
    <div className="whitespace-pre-wrap break-words text-[13px] font-medium leading-7 text-zinc-200">
        {content}
    </div>
);

const ChatMessageComponent: React.FC<ChatMessageProps> = ({
    message,
    pendingActions,
    onApproveCommand,
    onSkipCommand,
    onApproveSingleCommand,
    onSkipSingleCommand,
    isContinued = false,
    isActive = false,
    onUndoTool,
    onStopCommand,
    onOpenFile,
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

    const hasReasoning = !!message.reasoning || message.blocks?.some(b => b.type === 'reasoning');
    const stream = message.streaming;
    const hasChunkCounter = isAssistant && !!stream && stream.seq > 0;
    const streamAgeMs = stream ? Date.now() - stream.lastSeqAt : 0;
    const streamElapsedSec = stream
        ? ((stream.endTime ?? Date.now()) - stream.startTime) / 1000
        : 0;

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
    const imageAttachments = (message.images || []).flatMap((image, index) => {
        const { fullUrl, previewUrl, name } = resolveImageUrls(image);
        if (!fullUrl) return [];
        return [{
            id: `${message.id || 'msg'}-image-${index}`,
            fullUrl,
            previewUrl,
            name: name || `Attachment ${index + 1}`
        }];
    });
    const renderSegments = useMemo<RenderSegment[]>(() => {
        if (!message.blocks || message.blocks.length === 0) {
            return [];
        }

        const segments: RenderSegment[] = [];
        let index = 0;

        while (index < message.blocks.length) {
            const block = message.blocks[index];
            if (block.type !== 'tool_call' && block.type !== 'command_execution') {
                segments.push({ kind: 'block', block, index });
                index += 1;
                continue;
            }

            const items: ActivityGroupItem[] = [];
            let cursor = index;

            while (cursor < message.blocks.length) {
                const candidate = message.blocks[cursor];
                if (candidate.type === 'tool_call') {
                    const toolCall = message.tool_calls?.find((tc) => tc.id === candidate.id);
                    const isRunCommand = toolCall?.function.name === 'run_command';
                    const hasPendingApproval = !!pendingActions && pendingActions.length > 0;
                    const hasExecutionBlock = message.blocks?.some((b) => b.type === 'command_execution' && b.id === candidate.id);
                    if (toolCall && !(isRunCommand && hasPendingApproval) && !(isRunCommand && hasExecutionBlock)) {
                        items.push({ kind: 'tool_call', id: candidate.id, toolCall });
                    }
                    cursor += 1;
                    continue;
                }

                if (candidate.type === 'command_execution') {
                    const commandExecution = message.commandExecutions?.find((execution) => execution.id === candidate.id);
                    if (commandExecution) {
                        items.push({ kind: 'command_execution', id: candidate.id, commandExecution });
                    }
                    cursor += 1;
                    continue;
                }

                break;
            }

            if (items.length > 0) {
                segments.push({
                    kind: 'activity_group',
                    id: items.map((item) => item.id).join(':'),
                    items,
                });
            }

            index = cursor;
        }

        return segments;
    }, [message.blocks, message.commandExecutions, message.tool_calls, pendingActions]);

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
                    }
                }
            );
        }

        showMenu({ x: e.clientX, y: e.clientY }, items);
    }, [message, isUser, isAssistant, showMenu]);

    return (
        <div
            className={`group px-3 ${isContinued ? 'pt-0.5 pb-1.5' : 'pt-3 pb-3'} ${isTool ? 'opacity-70' : ''}`}
            onContextMenu={handleContextMenu}
        >
            <div className={`relative flex gap-2.5 rounded-xl border px-3 py-2.5 transition-colors ${
                isUser
                    ? 'border-zinc-800/80 bg-zinc-900/55 shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]'
                    : isAssistant
                        ? 'border-zinc-800/70 bg-zinc-950/35 shadow-[0_12px_32px_rgba(0,0,0,0.12)]'
                        : 'border-zinc-800/70 bg-zinc-950/40'
            }`}>
                <div className="w-7 shrink-0 flex flex-col items-center pt-0.5">
                    {!isContinued ? (
                        <div className="opacity-90 transition-opacity group-hover:opacity-100">
                            {isUser && <div className="flex h-7 w-7 items-center justify-center rounded-xl border border-zinc-700/80 bg-zinc-800/80 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]"><User className="h-3.5 w-3.5 text-zinc-200" /></div>}
                            {isAssistant && <div className="flex h-7 w-7 items-center justify-center rounded-xl border border-zinc-700/70 bg-zinc-900/70 shadow-[inset_0_1px_0_rgba(255,255,255,0.04)]"><Bot className="h-3.5 w-3.5 text-zinc-200" /></div>}
                            {isSystem && <div className="flex h-7 w-7 items-center justify-center rounded-xl border border-yellow-500/20 bg-yellow-500/10"><Terminal className="h-3.5 w-3.5 text-yellow-500" /></div>}
                            {isTool && <div className="flex h-7 w-7 items-center justify-center rounded-xl border border-purple-500/20 bg-purple-500/10"><Terminal className="h-3.5 w-3.5 text-purple-400" /></div>}
                        </div>
                    ) : (
                        <div className="h-full w-px rounded-full bg-zinc-800/70" />
                    )}
                </div>

                <div className="min-w-0 flex-1 overflow-hidden space-y-1.5">
                    {!isContinued && (
                        <div className="mb-0.5 flex min-h-5 items-center gap-2">
                            <span className={`rounded-md border px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.16em] ${
                                isUser
                                    ? 'border-zinc-700/80 bg-zinc-800/70 text-zinc-300'
                                    : isAssistant
                                        ? 'border-zinc-800/80 bg-zinc-900/50 text-zinc-400'
                                        : isSystem
                                            ? 'border-yellow-500/20 bg-yellow-500/10 text-yellow-300'
                                            : 'border-purple-500/20 bg-purple-500/10 text-purple-300'
                            }`}>
                                {isUser ? 'You' : (isAssistant ? 'Assistant' : message.role)}
                            </span>
                            {isActive && isAssistant && (
                                <span className="inline-flex items-center gap-1 rounded-md border border-zinc-800/80 bg-zinc-900/50 px-1.5 py-0.5 text-[9px] font-medium text-zinc-400">
                                    <Loader2 className="h-2.5 w-2.5 animate-spin" />
                                    Live
                                </span>
                            )}
                            {isTool && message.tool_call_id && (
                                <span className="rounded-md border border-zinc-800 bg-zinc-950 px-1.5 py-0.5 text-[9px] font-mono text-zinc-500">
                                    {message.tool_call_id.slice(0, 8)}
                                </span>
                            )}
                        </div>
                    )}

                {imageAttachments.length > 0 && (
                    <div className="mb-2">
                        <div className="flex flex-wrap gap-2">
                            {imageAttachments.map((image) => (
                                <a
                                    key={image.id}
                                    href={image.fullUrl}
                                    target="_blank"
                                    rel="noreferrer"
                                    className="block max-w-[160px]"
                                >
                                    <div className="overflow-hidden rounded-md border border-zinc-800/60 bg-zinc-900/40">
                                        <img
                                            src={image.previewUrl}
                                            alt={image.name}
                                            loading="lazy"
                                            className="w-full max-h-32 object-contain"
                                        />
                                    </div>
                                    {image.name && (
                                        <div className="mt-1 text-[10px] text-zinc-500 truncate">
                                            {image.name}
                                        </div>
                                    )}
                                </a>
                            ))}
                        </div>
                    </div>
                )}

                {isUser && message.mentions && message.mentions.length > 0 && (
                    <ReferencedPathsDisplay mentions={message.mentions} onOpenFile={onOpenFile} />
                )}

                {/* Thinking indicator for slow models - show when active but no content yet */}
                {/* DISABLED: Investigating raw reasoning output
                {isActive && isAssistant && !hasContent && !hasReasoning && !message.progress && (
                    <div className="flex items-center gap-2 py-2 text-zinc-500">
                        <Loader2 className="w-3.5 h-3.5 animate-spin text-emerald-500/70" />
                        <span className="text-[11px] font-mono">Thinking...</span>
                    </div>
                )}
                */}

                {/* Render Interleaved Blocks if available, else Legacy Fallback */}
                {message.blocks && message.blocks.length > 0 ? (
                    <>
                        {(() => {
                            // Find the index of the last reasoning block
                            const lastReasoningIdx = message.blocks!.reduce((lastIdx, block, idx) => {
                                return block.type === 'reasoning' ? idx : lastIdx;
                            }, -1);
                            
                            return renderSegments.map((segment) => {
                                if (segment.kind === 'activity_group') {
                                    return (
                                        <ActivityGroupDisplay
                                            key={segment.id}
                                            items={segment.items}
                                            pendingActions={pendingActions}
                                            onUndoTool={onUndoTool}
                                            onStopCommand={onStopCommand}
                                            onOpenFile={onOpenFile}
                                        />
                                    );
                                }

                                const { block, index: idx } = segment;
                                if (block.type === 'reasoning') {
                                    // Only the last reasoning block is active
                                    const isReasoningActive = isActive && idx === lastReasoningIdx;
                                    return (
                                        <ReasoningBlock
                                            key={block.id || `reasoning-${idx}`}
                                            content={block.content}
                                            isActive={isReasoningActive}
                                            hasContent={false} // Interleaved reasoning acts somewhat independent
                                        />
                                    );
                                } else if (block.type === 'text') {
                                return (
                                    <div key={block.id || `text-${idx}`} className="mb-2 select-text">
                                        {isActive ? (
                                            <StreamingTextPreview content={block.content} />
                                        ) : (
                                            <MarkdownRenderer content={block.content} />
                                        )}
                                    </div>
                                );
                            } else if (block.type === 'todo') {
                                // Todo blocks are now rendered in the persistent TaskPanel
                                return null;
                            } else if (block.type === 'plan_summary') {
                                // Compact summary of a completed plan
                                if (!message.planSummary) return null;
                                return (
                                    <PlanSummaryDisplay
                                        key={block.id}
                                        todos={message.planSummary.todos}
                                    />
                                );
                            } else if (block.type === 'research_progress') {
                                // Progress is rendered in the bottom chat panel indicator only.
                                return null;
                            }
                            return null;
                            });
                        })()}

                        {/* Pending Actions (Command Approval) */}
                        {pendingActions && pendingActions.length > 0 && onApproveCommand && onSkipCommand && (
                            <div className="mb-3">
                                <CommandApprovalCard
                                    actions={pendingActions}
                                    onRun={onApproveCommand}
                                    onSkip={onSkipCommand}
                                    onRunSingle={onApproveSingleCommand}
                                    onSkipSingle={onSkipSingleCommand}
                                />
                            </div>
                        )}


                        {/* Legacy: Render commandExecutions that don't have block entries (backward compat) */}
                        {message.commandExecutions && message.commandExecutions.length > 0 && (
                            (() => {
                                const blockIds = new Set((message.blocks || []).filter(b => b.type === 'command_execution').map(b => b.id));
                                const orphanedCmds = message.commandExecutions.filter(c => !blockIds.has(c.id));
                                if (orphanedCmds.length === 0) return null;
                                return (
                                    <div className="mt-3 space-y-2">
                                        {orphanedCmds.map((cmd, idx) => (
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
                                );
                            })()
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
                                        {isActive ? (
                                            <StreamingTextPreview content={initialText} />
                                        ) : (
                                            <MarkdownRenderer content={initialText} />
                                        )}
                                    </div>
                                )}
                                {pendingActions && pendingActions.length > 0 && onApproveCommand && onSkipCommand && (
                                    <div className="mb-3">
                                        <CommandApprovalCard
                                            actions={pendingActions}
                                            onRun={onApproveCommand}
                                            onSkip={onSkipCommand}
                                            onRunSingle={onApproveSingleCommand}
                                            onSkipSingle={onSkipSingleCommand}
                                        />
                                    </div>
                                )}
                                {hasToolCalls && (
                                    <div className="mb-3 space-y-2">
                                        {toolCalls
                                            .filter(call => {
                                                // Skip run_command when pending approval - CommandApprovalCard handles it
                                                if (call.function.name === 'run_command' && pendingActions && pendingActions.length > 0) {
                                                    return false;
                                                }
                                                return true;
                                            })
                                            .map((call, idx) => (
                                                <ToolCallDisplay
                                                    key={`${call.id}-${idx}`}
                                                    toolCall={call}
                                                    status={call.status || 'executing'}
                                                    result={call.result}
                                                    onStopCommand={
                                                        call.function.name === 'run_command' && onStopCommand
                                                            ? (() => onStopCommand(call.id))
                                                            : undefined
                                                    }
                                                    onUndo={
                                                        onUndoTool && REVERTIBLE_TOOLS.has(call.function.name)
                                                            ? (() => onUndoTool(call.id))
                                                            : undefined
                                                    }
                                                    onOpenFile={onOpenFile}
                                                />
                                            ))}
                                    </div>
                                )}
                                {finalText && (
                                    <div className="select-text">
                                        {isActive ? (
                                            <StreamingTextPreview content={finalText} />
                                        ) : (
                                            <MarkdownRenderer content={finalText} />
                                        )}
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

                {hasChunkCounter && (
                    <div className="mt-3 inline-flex items-center gap-2 rounded-full border border-zinc-800/80 bg-zinc-950/70 px-2.5 py-1 text-[10px] font-mono text-zinc-500">
                        <span>{stream!.seq} chunks</span>
                        <span className="text-zinc-700">•</span>
                        <span>
                            {stream!.endTime
                                ? `${streamElapsedSec.toFixed(1)}s`
                                : (isActive && streamAgeMs > 5000 ? 'waiting...' : 'streaming...')}
                        </span>
                    </div>
                )}
            </div>
        </div>
        </div>
    );
};

// Custom comparison for ChatMessage - only re-render when meaningful props change
export const ChatMessage = React.memo(ChatMessageComponent, (prevProps, nextProps) => {
    // Quick bail-out checks for primitive props
    if (prevProps.isContinued !== nextProps.isContinued) return false;
    if (prevProps.isActive !== nextProps.isActive) return false;
    
    // Message content comparison - the most important check
    const prevMsg = prevProps.message;
    const nextMsg = nextProps.message;
    if (prevMsg.id !== nextMsg.id) return false;
    if (prevMsg.content !== nextMsg.content) return false;
    if (prevMsg.reasoning !== nextMsg.reasoning) return false;
    if ((prevMsg.streaming?.seq ?? null) !== (nextMsg.streaming?.seq ?? null)) return false;
    if ((prevMsg.streaming?.endTime ?? null) !== (nextMsg.streaming?.endTime ?? null)) return false;
    const prevMentionSignature = (prevMsg.mentions || []).map((mention) => `${mention.kind}:${mention.path}:${mention.is_dir}`).join('|');
    const nextMentionSignature = (nextMsg.mentions || []).map((mention) => `${mention.kind}:${mention.path}:${mention.is_dir}`).join('|');
    if (prevMentionSignature !== nextMentionSignature) return false;
    if (prevMsg.tool_calls?.length !== nextMsg.tool_calls?.length) return false;
    if (prevMsg.blocks?.length !== nextMsg.blocks?.length) return false;
    
    // Check tool call statuses (important for showing execution state)
    if (prevMsg.tool_calls && nextMsg.tool_calls) {
        for (let i = 0; i < prevMsg.tool_calls.length; i++) {
            if (prevMsg.tool_calls[i].status !== nextMsg.tool_calls[i].status) return false;
        }
    }
    
    // Pending actions - check both reference AND presence change
    const prevHasPending = prevProps.pendingActions && prevProps.pendingActions.length > 0;
    const nextHasPending = nextProps.pendingActions && nextProps.pendingActions.length > 0;
    if (prevHasPending !== nextHasPending) return false;
    if (prevProps.pendingActions !== nextProps.pendingActions) return false;
    
    // Callbacks - check presence change (undefined vs function)
    const prevHasApprove = !!prevProps.onApproveCommand;
    const nextHasApprove = !!nextProps.onApproveCommand;
    const prevHasSkip = !!prevProps.onSkipCommand;
    const nextHasSkip = !!nextProps.onSkipCommand;
    if (prevHasApprove !== nextHasApprove) return false;
    if (prevHasSkip !== nextHasSkip) return false;
    
    return true;
});
