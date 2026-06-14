import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { ChatMessage as ChatMessageType, ChatImage, HookApprovalRequest, ImageAttachment } from '../types/chat';
import { User, Bot, Terminal, Brain, ChevronDown, ChevronRight, Loader2, Copy, RotateCcw, Pencil, MessageSquare, Check, FileText, Folder } from 'lucide-react';
import { ToolCallDisplay } from './ToolCallDisplay';
import { CommandOutputDisplay } from './CommandOutputDisplay';
import { CommandApprovalCard } from './CommandApprovalCard';
import { HookApprovalCard } from './HookApprovalCard';
import { useContextMenu, ContextMenuItem } from './ui/ContextMenu';
import { MarkdownRenderer, StreamingMarkdownRenderer } from './MarkdownRenderer';
import { deriveChatWorkEntries, deriveMessageRenderSegments, type ChatWorkEntry, type DerivedActivityGroupItem } from '../utils/chatTimeline';
import { getCopyableMessageContent } from '../utils/chatMessageCopy';

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

const LARGE_USER_MESSAGE_CHAR_THRESHOLD = 12000;
const LARGE_USER_MESSAGE_LINE_THRESHOLD = 220;
const REASONING_SCROLL_SYNC_INTERVAL_MS = 100;

const PlainTextMessage = React.memo<{ content: string }>(({ content }) => (
    <div className="select-text">
        <pre className="m-0 whitespace-pre-wrap wrap-break-word bg-transparent font-sans text-(--fg-primary)" style={{ fontSize: 'var(--chat-content-font-size, 13px)', lineHeight: 1.65 }}>
            {content}
        </pre>
    </div>
));
PlainTextMessage.displayName = 'PlainTextMessage';

function shouldUsePlainTextForLargeUserMessage(content: string): boolean {
    if (!content) {
        return false;
    }

    if (content.length >= LARGE_USER_MESSAGE_CHAR_THRESHOLD) {
        return true;
    }

    const lineCount = (content.match(/\n/g)?.length ?? 0) + 1;
    return lineCount >= LARGE_USER_MESSAGE_LINE_THRESHOLD;
}

function normalizeReasoningDisplayContent(content: string): string {
    if (!content) {
        return '';
    }

    if (content.search(/\[\/?THINKING\]/i) === -1) {
        return content;
    }

    return content
        .replace(/\[THINKING\]/gi, '')
        .replace(/\[\/THINKING\]/gi, '')
        .trim();
}

const ReasoningBlock: React.FC<{ content: string; isActive?: boolean; hasContent?: boolean }> = ({ content, isActive, hasContent }) => {
    const { t } = useTranslation();
    const [isExpanded, setIsExpanded] = useState(true); // Start expanded
    const [userToggled, setUserToggled] = useState(false); // Track if user manually toggled
    const contentRef = useRef<HTMLDivElement>(null);
    const wasActiveRef = useRef(isActive);
    const hadContentRef = useRef(hasContent);
    const reasoningScrollFrameRef = useRef<number | null>(null);
    const reasoningScrollTimeoutRef = useRef<number | null>(null);
    const lastReasoningScrollAtRef = useRef(0);

    const displayContent = useMemo(() => normalizeReasoningDisplayContent(content), [content]);

    const cancelReasoningScrollSync = useCallback(() => {
        if (reasoningScrollFrameRef.current !== null) {
            cancelAnimationFrame(reasoningScrollFrameRef.current);
            reasoningScrollFrameRef.current = null;
        }
        if (reasoningScrollTimeoutRef.current !== null) {
            window.clearTimeout(reasoningScrollTimeoutRef.current);
            reasoningScrollTimeoutRef.current = null;
        }
    }, []);

    // Auto-expand while reasoning is active, auto-collapse when reasoning ends
    useEffect(() => {
        if (isActive && !wasActiveRef.current && !userToggled) {
            setIsExpanded(true);
        }

        if (!isActive && wasActiveRef.current && isExpanded && !userToggled) {
            setIsExpanded(false);
        }

        wasActiveRef.current = isActive;
        hadContentRef.current = hasContent;
    }, [isActive, hasContent, isExpanded, userToggled]);

    // Auto-scroll to bottom while streaming, capped to avoid repeated layout work.
    useEffect(() => {
        if (!isExpanded || !isActive || !contentRef.current) {
            cancelReasoningScrollSync();
            return;
        }

        if (reasoningScrollFrameRef.current !== null || reasoningScrollTimeoutRef.current !== null) {
            return;
        }

        const runScroll = () => {
            reasoningScrollFrameRef.current = null;
            const element = contentRef.current;
            if (!element) return;

            element.scrollTop = element.scrollHeight;
            lastReasoningScrollAtRef.current = Date.now();
        };

        const scheduleScrollFrame = () => {
            reasoningScrollFrameRef.current = requestAnimationFrame(runScroll);
        };

        const elapsedSinceScroll = Date.now() - lastReasoningScrollAtRef.current;
        const delayMs = Math.max(0, REASONING_SCROLL_SYNC_INTERVAL_MS - elapsedSinceScroll);
        if (delayMs === 0) {
            scheduleScrollFrame();
            return;
        }

        reasoningScrollTimeoutRef.current = window.setTimeout(() => {
            reasoningScrollTimeoutRef.current = null;
            scheduleScrollFrame();
        }, delayMs);
    }, [cancelReasoningScrollSync, displayContent, isExpanded, isActive]);

    useEffect(() => cancelReasoningScrollSync, [cancelReasoningScrollSync]);

    // DEBUG: Show raw content even if empty after cleaning, to see what's being suppressed
    if (!displayContent) return null;

    const handleToggle = () => {
        setIsExpanded(!isExpanded);
        setUserToggled(true); // Mark that user has manually controlled this
    };

    const isStreaming = isActive && !hasContent;
    const containerStyle = isStreaming
        ? {
            backgroundColor: 'color-mix(in srgb, var(--bg-surface) 88%, var(--bg-app))',
            borderColor: 'color-mix(in srgb, var(--accent-ai) 26%, var(--border-default))',
            boxShadow: 'var(--shadow-md)'
        }
        : {
            backgroundColor: 'color-mix(in srgb, var(--bg-surface) 70%, var(--bg-app))',
            borderColor: 'color-mix(in srgb, var(--border-default) 92%, transparent)',
            boxShadow: 'none',
            opacity: 0.9
        };
    const headerStyle = {
        backgroundColor: isStreaming
            ? 'color-mix(in srgb, var(--accent-ai) 8%, var(--bg-surface))'
            : 'color-mix(in srgb, var(--bg-surface) 54%, var(--bg-app))'
    };
    const contentStyle = {
        backgroundColor: isStreaming
            ? 'color-mix(in srgb, var(--bg-app) 16%, var(--bg-surface))'
            : 'color-mix(in srgb, var(--bg-app) 10%, var(--bg-surface))',
        borderColor: isStreaming
            ? 'color-mix(in srgb, var(--accent-ai) 16%, var(--border-subtle))'
            : 'color-mix(in srgb, var(--border-default) 72%, transparent)'
    };
    const contentTextStyle = {
        color: isStreaming
            ? 'color-mix(in srgb, var(--fg-primary) 88%, var(--accent-ai))'
            : 'var(--fg-secondary)'
    };

    return (
        <div
            className="my-2 overflow-hidden rounded-[calc(var(--panel-radius)*0.75)] border transition-[border-color,background-color,box-shadow,opacity] duration-200"
            style={containerStyle}
        >
            <button
                type="button"
                onClick={handleToggle}
                className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-(--bg-surface-hover)/45"
                style={headerStyle}
            >
                <div className="flex items-center gap-2 flex-1 min-w-0">
                    <Brain aria-hidden="true" className={`h-3 w-3 shrink-0 ${isStreaming ? 'animate-pulse text-(--accent-ai)' : 'text-(--fg-tertiary)'}`} />
                    <span className={`shrink-0 font-mono text-[9px] uppercase tracking-[0.16em] ${isStreaming ? 'text-(--accent-ai)' : 'text-(--fg-tertiary)'
                        }`}>
                        {t('chatMessage.reasoning')}
                    </span>
                </div>
                {isStreaming && (
                    <Loader2 aria-hidden="true" className="mr-1 h-2.5 w-2.5 shrink-0 animate-spin text-(--accent-ai)" />
                )}
                {isExpanded ? (
                    <ChevronDown aria-hidden="true" className="h-3 w-3 shrink-0 text-(--fg-secondary)" />
                ) : (
                    <ChevronRight aria-hidden="true" className="h-3 w-3 shrink-0 text-(--fg-secondary)" />
                )}
            </button>

            {isExpanded && (
                <div
                    ref={contentRef}
                    className="max-h-48 overflow-x-hidden overflow-y-auto border-t px-3 py-2"
                    style={contentStyle}
                >
                    <div
                        className="select-text overflow-wrap-anywhere text-[11px] leading-relaxed"
                        style={contentTextStyle}
                    >
                        {isActive ? (
                            <StreamingMarkdownRenderer content={displayContent} />
                        ) : (
                            <MarkdownRenderer content={displayContent} />
                        )}
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
    const { t } = useTranslation();
    const [isExpanded, setIsExpanded] = useState(false);
    const completedCount = todos.filter(t => t.status === 'completed').length;

    return (
        <div className="my-2">
            <button
                type="button"
                onClick={() => setIsExpanded(!isExpanded)}
                className="flex items-center gap-2 px-3 py-1.5 rounded-[calc(var(--panel-radius)*0.45)] bg-[color-mix(in_srgb,var(--accent-mention)_5%,transparent)] border border-(--accent-mention)/20 hover:bg-[color-mix(in_srgb,var(--accent-mention)_10%,transparent)] transition-colors text-left w-full"
            >
                <Check className="w-3.5 h-3.5 text-(--accent-mention) shrink-0" aria-hidden="true" />
                <span className="text-[11px] text-(--accent-mention) font-medium">
                    {t('chatMessage.planCompleted', { completed: completedCount, total: todos.length })}
                </span>
                {isExpanded ? (
                    <ChevronDown className="w-3 h-3 text-(--fg-tertiary) ml-auto shrink-0" aria-hidden="true" />
                ) : (
                    <ChevronRight className="w-3 h-3 text-(--fg-tertiary) ml-auto shrink-0" aria-hidden="true" />
                )}
            </button>
            {isExpanded && (
                <div className="mt-1 px-3 py-2 rounded-[calc(var(--panel-radius)*0.45)] bg-[color-mix(in_srgb,var(--accent-mention)_5%,transparent)] border border-(--accent-mention)/10 space-y-0.5">
                    {todos.map((todo, idx) => (
                        <div key={idx} className="flex items-center gap-2 text-[11px]">
                            <Check className="w-3 h-3 text-(--accent-mention)/60 shrink-0" aria-hidden="true" />
                            <span className="text-(--fg-tertiary) line-through">{todo.content}</span>
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
    const { t } = useTranslation();
    if (mentions.length === 0) {
        return null;
    }

    return (
        <div className="overflow-hidden rounded-[calc(var(--panel-radius)*0.75)] border border-(--accent-mention)/15 bg-[color-mix(in_srgb,var(--accent-mention)_6%,transparent)]">
            <div className="flex items-center gap-2 border-b border-(--accent-mention)/10 px-3 py-1.5 text-[10px] text-(--accent-mention)/90">
                <span className="font-semibold uppercase tracking-[0.16em]">{t('chat.referencedPaths')}</span>
                <span className="text-(--accent-mention)/60">{mentions.length}</span>
            </div>
            <div className="flex flex-wrap gap-1.5 px-3 py-2">
                {mentions.map((mention) => {
                    const isClickable = !mention.is_dir && !!onOpenFile;
                    const content = (
                        <>
                            {mention.is_dir ? (
                                <Folder className="h-3 w-3 shrink-0 text-(--accent-mention)/80" aria-hidden="true" />
                            ) : (
                                <FileText className="h-3 w-3 shrink-0 text-(--accent-mention)/80" aria-hidden="true" />
                            )}
                            <span className="truncate">{mention.path}</span>
                        </>
                    );

                    if (!isClickable) {
                        return (
                            <div
                                key={`${mention.kind}:${mention.path}`}
                                className="inline-flex max-w-full items-center gap-1.5 rounded-[calc(var(--panel-radius)*0.45)] border border-(--accent-mention)/15 bg-(--bg-app)/30 px-2 py-1 text-[10px] text-(--fg-secondary)"
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
                            className="inline-flex max-w-full items-center gap-1.5 rounded-[calc(var(--panel-radius)*0.45)] border border-(--accent-mention)/15 bg-(--bg-app)/30 px-2 py-1 text-[10px] text-(--fg-secondary) transition-colors hover:bg-[color-mix(in_srgb,var(--accent-mention)_10%,transparent)] hover:text-(--fg-bright)"
                            title={t('chatMessage.openPath', { path: mention.path })}
                            aria-label={t('chatMessage.openPath', { path: mention.path })}
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
    DerivedActivityGroupItem;

function getApprovalActivityTargetKey(id: string): string {
    return `approval:${id}`;
}

function getToolActivityTargetKey(id: string): string {
    return `tool:${id}`;
}

function compactWorkEntryToneClass(entry: ChatWorkEntry): string {
    if (entry.status === 'executing' || entry.status === 'pending') {
        return 'bg-(--accent-ai)';
    }
    if (entry.tone === 'error') {
        return 'bg-(--state-danger)';
    }
    return 'bg-(--accent-mention)';
}

const CompactWorkLog: React.FC<{
    entries: ChatWorkEntry[];
    showDetails: boolean;
    detailsLockedOpen: boolean;
    onToggleDetails: () => void;
}> = React.memo(({ entries, showDetails, detailsLockedOpen, onToggleDetails }) => {
    const [isExpanded, setIsExpanded] = useState(false);
    if (entries.length === 0) {
        return null;
    }

    const visibleEntries = isExpanded ? entries : entries.slice(0, 3);
    const hiddenCount = entries.length - visibleEntries.length;

    return (
        <div className="rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface)/45 px-2.5 py-2">
            <button
                type="button"
                className="flex w-full items-center justify-between gap-3 text-left"
                onClick={() => setIsExpanded((value) => !value)}
                aria-expanded={isExpanded}
            >
                <span className="flex min-w-0 items-center gap-2">
                    <Terminal aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-(--fg-tertiary)" />
                    <span className="text-[9px] font-semibold uppercase tracking-[0.16em] text-(--fg-tertiary)">
                        Work log ({entries.length})
                    </span>
                    {!isExpanded && (
                        <span className="min-w-0 truncate text-[10px] text-(--fg-secondary)">
                            {entries.slice(0, 2).map((entry) => entry.label).join(', ')}
                        </span>
                    )}
                </span>
                {isExpanded ? (
                    <ChevronDown aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-(--fg-tertiary)" />
                ) : (
                    <ChevronRight aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-(--fg-tertiary)" />
                )}
            </button>
            {(isExpanded || entries.length > 3) && (
                <div className="mt-1.5 space-y-1">
                    {visibleEntries.map((entry) => (
                        <div key={entry.id} className="flex min-w-0 items-center gap-2 rounded-[calc(var(--panel-radius)*0.35)] px-1 py-0.5">
                            <span aria-hidden="true" className={`h-1.5 w-1.5 shrink-0 rounded-full ${compactWorkEntryToneClass(entry)}`} />
                            <span className="shrink-0 text-[10px] font-medium text-(--fg-secondary)">
                                {entry.label}
                            </span>
                            {entry.detail && (
                                <span className="min-w-0 truncate text-[10px] font-mono text-(--fg-tertiary)" title={entry.detail}>
                                    {entry.detail}
                                </span>
                            )}
                        </div>
                    ))}
                    {!isExpanded && hiddenCount > 0 && (
                        <div className="px-1 text-[10px] text-(--fg-tertiary)">
                            +{hiddenCount} more
                        </div>
                    )}
                </div>
            )}
            <div className="mt-1.5 flex items-center justify-end">
                <button
                    type="button"
                    className="rounded-[calc(var(--panel-radius)*0.35)] px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.12em] text-(--fg-tertiary) transition-colors hover:text-(--fg-secondary) disabled:cursor-default disabled:opacity-60"
                    onClick={onToggleDetails}
                    disabled={detailsLockedOpen}
                    aria-pressed={showDetails || detailsLockedOpen}
                >
                    {detailsLockedOpen ? 'Details visible' : showDetails ? 'Hide details' : 'Show details'}
                </button>
            </div>
        </div>
    );
});

CompactWorkLog.displayName = 'CompactWorkLog';

const ActivityGroupDisplay: React.FC<{
    items: ActivityGroupItem[];
    pendingActions?: import('../types/events').StructuredAction[] | null;
    pendingApprovalRequest?: HookApprovalRequest | null;
    onApproveCommand?: () => void;
    onSkipCommand?: () => void;
    onApproveApprovalRequest?: () => void;
    onDenyApprovalRequest?: () => void;
    onApproveSingleCommand?: (callId: string) => void;
    onSkipSingleCommand?: (callId: string) => void;
    onUndoTool?: (toolCallId: string) => void;
    onStopCommand?: (callId: string) => void;
    onOpenFile?: (path: string) => void;
    workspaceRoot?: string | null;
    registerActivityTarget?: (targetKey: string, element: HTMLDivElement | null) => void;
}> = ({ items, pendingActions, pendingApprovalRequest, onApproveCommand, onSkipCommand, onApproveApprovalRequest, onDenyApprovalRequest, onApproveSingleCommand, onSkipSingleCommand, onUndoTool, onStopCommand, onOpenFile, workspaceRoot, registerActivityTarget }) => {
    return (
        <div className="space-y-1.5">
            {items.map((item) => {
                if (item.kind === 'tool_call') {
                    const toolCall = item.toolCall;
                    const matchingPendingAction = pendingActions?.find((action) => action.id === toolCall.id);
                    const matchingApprovalRequest = pendingApprovalRequest?.toolCallId === toolCall.id ? pendingApprovalRequest : null;
                    if (toolCall.function.name === 'run_command' && matchingPendingAction && onApproveCommand && onSkipCommand) {
                        return (
                            <div
                                key={item.id}
                                ref={(element) => registerActivityTarget?.(getApprovalActivityTargetKey(matchingPendingAction.id), element)}
                            >
                                <CommandApprovalCard
                                    actions={[matchingPendingAction]}
                                    onRun={onApproveCommand}
                                    onSkip={onSkipCommand}
                                    onRunSingle={onApproveSingleCommand}
                                    onSkipSingle={onSkipSingleCommand}
                                />
                            </div>
                        );
                    }
                    if (matchingApprovalRequest && onApproveApprovalRequest && onDenyApprovalRequest) {
                        return (
                            <div
                                key={item.id}
                                ref={(element) => registerActivityTarget?.(getApprovalActivityTargetKey(matchingApprovalRequest.toolCallId), element)}
                            >
                                <HookApprovalCard
                                    request={matchingApprovalRequest}
                                    onApprove={onApproveApprovalRequest}
                                    onDeny={onDenyApprovalRequest}
                                />
                            </div>
                        );
                    }
                    return (
                        <div
                            key={item.id}
                            ref={(element) => registerActivityTarget?.(getToolActivityTargetKey(toolCall.id), element)}
                        >
                            <ToolCallDisplay
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
                                workspaceRoot={workspaceRoot}
                            />
                        </div>
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
    );
};

interface ChatMessageProps {
    message: ChatMessageType;
    pendingActions?: import('../types/events').StructuredAction[];
    pendingApprovalRequest?: HookApprovalRequest;
    onApproveCommand?: () => void;
    onSkipCommand?: () => void;
    onApproveApprovalRequest?: () => void;
    onDenyApprovalRequest?: () => void;
    onApproveSingleCommand?: (callId: string) => void;
    onSkipSingleCommand?: (callId: string) => void;
    isContinued?: boolean; // For visual grouping
    isActive?: boolean; // Is this the currently streaming message?
    onUndoTool?: (toolCallId: string) => void;
    onStopCommand?: (callId: string) => void;
    onOpenFile?: (path: string) => void;
    workspaceRoot?: string | null;
    onEditMessage?: () => void;
    registerActivityTarget?: (targetKey: string, element: HTMLDivElement | null) => void;
    showInlineWorkLog?: boolean;
    workDetailsVisible?: boolean;
    onToggleWorkDetails?: () => void;
}

const ChatMessageComponent: React.FC<ChatMessageProps> = ({
    message,
    pendingActions,
    pendingApprovalRequest,
    onApproveCommand,
    onSkipCommand,
    onApproveApprovalRequest,
    onDenyApprovalRequest,
    onApproveSingleCommand,
    onSkipSingleCommand,
    isContinued = false,
    isActive = false,
    onUndoTool,
    onStopCommand,
    onOpenFile,
    workspaceRoot,
    onEditMessage,
    registerActivityTarget,
    showInlineWorkLog = true,
    workDetailsVisible,
    onToggleWorkDetails,
}) => {
    const { t } = useTranslation();
    const isUser = message.role === 'User';
    const isSystem = message.role === 'System';
    const isTool = message.role === 'Tool';
    const isAssistant = message.role === 'Assistant';
    const { showMenu } = useContextMenu();
    const [showDetailedWork, setShowDetailedWork] = useState(false);

    const hasReasoning = !!message.reasoning || message.blocks?.some(b => b.type === 'reasoning');
    const stream = message.streaming;
    const hasChunkCounter = isAssistant && !!stream && stream.seq > 0;
    const hasLiveTextStream = isAssistant
        && !!stream
        && !stream.endTime
        && (stream.activeKind === 'content' || stream.activeKind === 'reasoning');
    const hasStreamedTextBlock = isAssistant
        && !!stream
        && (stream.activeKind === 'content' || stream.activeKind === 'reasoning');
    const shouldUseStreamingMarkdown = hasStreamedTextBlock;
    const showAssistantLiveState = isAssistant && (isActive || hasLiveTextStream);
    const isActiveContentBlock = useCallback((blockId?: string) => (
        shouldUseStreamingMarkdown
        && stream?.activeKind === 'content'
        && !!blockId
        && stream.activeBlockId === blockId
    ), [shouldUseStreamingMarkdown, stream?.activeBlockId, stream?.activeKind]);
    const isLiveContentBlock = useCallback((blockId?: string) => (
        hasLiveTextStream
        && stream?.activeKind === 'content'
        && !!blockId
        && stream.activeBlockId === blockId
    ), [hasLiveTextStream, stream?.activeBlockId, stream?.activeKind]);
    const isActiveReasoningBlock = useCallback((blockId?: string) => (
        hasLiveTextStream
        && stream?.activeKind === 'reasoning'
        && !!blockId
        && stream.activeBlockId === blockId
    ), [hasLiveTextStream, stream?.activeBlockId, stream?.activeKind]);
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
    const renderTextContent = (
        content: string,
        useStreamingRenderer = shouldUseStreamingMarkdown,
        isAnimating = useStreamingRenderer && hasLiveTextStream,
    ) => {
        if (isUser && shouldUsePlainTextForLargeUserMessage(content)) {
            return <PlainTextMessage content={content} />;
        }

        return useStreamingRenderer
            ? <StreamingMarkdownRenderer content={content} isAnimating={isAnimating} />
            : <MarkdownRenderer content={content} />;
    };
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
    const renderSegments = useMemo(
        () => deriveMessageRenderSegments(message, pendingActions),
        [message, pendingActions]
    );
    const compactWorkEntries = useMemo(
        () => isAssistant ? deriveChatWorkEntries([message]) : [],
        [isAssistant, message],
    );
    const detailsLockedOpen = compactWorkEntries.some((entry) => entry.status === 'executing' || entry.status === 'pending')
        || !!pendingApprovalRequest
        || !!pendingActions?.length;
    const effectiveShowDetailedWork = workDetailsVisible ?? showDetailedWork;
    const shouldShowDetailedWork = compactWorkEntries.length === 0 || effectiveShowDetailedWork || detailsLockedOpen;
    const handleToggleWorkDetails = useCallback(() => {
        if (onToggleWorkDetails) {
            onToggleWorkDetails();
            return;
        }
        setShowDetailedWork((value) => !value);
    }, [onToggleWorkDetails]);
    const assistantCardStyle = isAssistant
        ? {
            backgroundColor: 'color-mix(in srgb, var(--bg-panel) 80%, var(--bg-app))',
            borderColor: 'color-mix(in srgb, var(--border-default) 88%, transparent)',
            boxShadow: 'var(--shadow-lg)'
        }
        : undefined;
    const userCardStyle = isUser
        ? {
            backgroundColor: 'color-mix(in srgb, var(--bg-surface) 92%, var(--bg-app))',
            borderColor: 'color-mix(in srgb, var(--border-default) 94%, transparent)',
            boxShadow: 'var(--shadow-sm)'
        }
        : undefined;
    const assistantIconStyle = isAssistant
        ? {
            backgroundColor: 'color-mix(in srgb, var(--bg-surface) 72%, var(--bg-app))',
            borderColor: 'color-mix(in srgb, var(--border-default) 92%, transparent)'
        }
        : undefined;
    const userIconStyle = isUser
        ? {
            backgroundColor: 'color-mix(in srgb, var(--bg-surface-hover) 82%, var(--bg-app))',
            borderColor: 'color-mix(in srgb, var(--border-default) 92%, transparent)'
        }
        : undefined;
    const assistantLabelStyle = isAssistant
        ? {
            backgroundColor: 'color-mix(in srgb, var(--bg-surface) 52%, var(--bg-app))',
            borderColor: 'color-mix(in srgb, var(--border-subtle) 100%, transparent)'
        }
        : undefined;
    const userLabelStyle = isUser
        ? {
            backgroundColor: 'color-mix(in srgb, var(--bg-surface-hover) 70%, var(--bg-app))',
            borderColor: 'color-mix(in srgb, var(--border-subtle) 100%, transparent)'
        }
        : undefined;
    const assistantLiveStyle = isAssistant
        ? {
            backgroundColor: 'color-mix(in srgb, var(--bg-surface) 45%, var(--bg-app))',
            borderColor: 'color-mix(in srgb, var(--accent-ai) 38%, transparent)'
        }
        : undefined;
    const assistantChunkCounterStyle = isAssistant
        ? {
            backgroundColor: stream?.endTime
                ? 'color-mix(in srgb, var(--bg-surface) 58%, var(--bg-app))'
                : 'color-mix(in srgb, var(--bg-surface) 48%, var(--bg-app))',
            borderColor: stream?.endTime
                ? 'color-mix(in srgb, var(--border-default) 94%, transparent)'
                : 'color-mix(in srgb, var(--accent-ai) 32%, var(--border-default))',
            color: stream?.endTime
                ? 'var(--fg-tertiary)'
                : 'color-mix(in srgb, var(--accent-ai) 70%, var(--fg-secondary))'
        }
        : undefined;
    const assistantChunkCounterDividerStyle = isAssistant
        ? {
            color: 'color-mix(in srgb, var(--border-default) 82%, var(--fg-tertiary))'
        }
        : undefined;
    const assistantDividerStyle = isAssistant
        ? {
            backgroundColor: 'color-mix(in srgb, var(--border-default) 88%, transparent)'
        }
        : undefined;
    const userDividerStyle = isUser
        ? {
            backgroundColor: 'color-mix(in srgb, var(--border-default) 88%, transparent)'
        }
        : undefined;
    const copyableMessageContent = getCopyableMessageContent(message);

    // Context menu for chat messages
    const handleContextMenu = useCallback((e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();

        const items: ContextMenuItem[] = [
            {
                id: 'copy-message',
                label: t('chat.contextMenu.copyMessage'),
                icon: <Copy className="w-4 h-4" />,
                shortcut: 'Ctrl+C',
                onClick: async () => {
                    try {
                        await navigator.clipboard.writeText(copyableMessageContent);
                    } catch (err) {
                        console.error('[Context] Failed to copy:', err);
                    }
                }
            },
            {
                id: 'copy-markdown',
                label: t('chat.contextMenu.copyAsMarkdown'),
                icon: <MessageSquare className="w-4 h-4" />,
                onClick: async () => {
                    try {
                        await navigator.clipboard.writeText(copyableMessageContent);
                    } catch (err) {
                        console.error('[Context] Failed to copy:', err);
                    }
                }
            },
        ];

        if (isUser && onEditMessage) {
            items.push(
                { id: 'div-1', label: '', divider: true },
                {
                    id: 'edit-message',
                    label: t('chat.contextMenu.editMessage'),
                    icon: <Pencil className="w-4 h-4" />,
                    onClick: onEditMessage
                }
            );
        }

        if (isAssistant) {
            items.push(
                { id: 'div-1', label: '', divider: true },
                {
                    id: 'regenerate',
                    label: t('chat.contextMenu.regenerateResponse'),
                    icon: <RotateCcw className="w-4 h-4" />,
                    onClick: () => {
                        // TODO: Implement regenerate
                    }
                }
            );
        }

        showMenu({ x: e.clientX, y: e.clientY }, items);
    }, [copyableMessageContent, isUser, isAssistant, onEditMessage, showMenu, t]);

    // Don't render Tool messages separately - they're shown in the tool call display
    // UNLESS this is a standalone tool message not handled by the previous assistant message.
    // However, for the "Single Turn" view, we rely on the Assistant message containing the calls.
    if (isTool) {
        return null;
    }

    return (
        <div
            className={`group px-1.5 ${isTool ? 'opacity-70' : ''}`}
            onContextMenu={handleContextMenu}
        >
            <div className={`relative w-full rounded-[calc(var(--panel-radius)*0.9)] border px-3 py-2.5 transition-colors ${
                isUser
                    ? ''
                    : isAssistant
                        ? ''
                        : 'border-(--accent-warning)/20 bg-[color-mix(in_srgb,var(--accent-warning)_8%,var(--bg-app))]'
            }`} style={isUser ? userCardStyle : assistantCardStyle}>
                <div className="min-w-0 overflow-hidden space-y-2">
                    {!isContinued && (
                        <div className="flex flex-col gap-2">
                            <div className="flex min-h-6 items-center gap-2">
                                <div className="opacity-90 transition-opacity group-hover:opacity-100">
                                    {isUser && <div className="flex h-6 w-6 items-center justify-center rounded-[calc(var(--panel-radius)*0.55)] border" style={userIconStyle}><User className="h-3 w-3 text-(--fg-secondary)" aria-hidden="true" /></div>}
                                    {isAssistant && <div className="flex h-6 w-6 items-center justify-center rounded-[calc(var(--panel-radius)*0.55)] border shadow-(--shadow-sm)" style={assistantIconStyle}><Bot className="h-3 w-3 text-(--accent-ai)" aria-hidden="true" /></div>}
                                    {isSystem && <div className="flex h-6 w-6 items-center justify-center rounded-[calc(var(--panel-radius)*0.55)] border border-(--accent-warning)/20 bg-[color-mix(in_srgb,var(--accent-warning)_10%,transparent)]"><Terminal className="h-3 w-3 text-(--accent-warning)" aria-hidden="true" /></div>}
                                    {isTool && <div className="flex h-6 w-6 items-center justify-center rounded-[calc(var(--panel-radius)*0.55)] border border-(--accent-planning)/20 bg-[color-mix(in_srgb,var(--accent-planning)_10%,transparent)]"><Terminal className="h-3 w-3 text-(--accent-planning)" aria-hidden="true" /></div>}
                                </div>
                                <span className={`rounded-[calc(var(--panel-radius)*0.35)] border px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.16em] ${
                                    isUser
                                        ? 'text-(--fg-tertiary)'
                                        : isAssistant
                                            ? 'text-(--fg-tertiary)'
                                            : isSystem
                                                ? 'border-(--accent-warning)/20 bg-[color-mix(in_srgb,var(--accent-warning)_10%,transparent)] text-(--accent-warning)'
                                                : 'border-(--accent-planning)/20 bg-[color-mix(in_srgb,var(--accent-planning)_10%,transparent)] text-(--accent-planning)'
                                }`} style={isUser ? userLabelStyle : assistantLabelStyle}>
                                    {isUser ? t('chatMessage.you') : (isAssistant ? t('chatMessage.assistant') : message.role)}
                                </span>
                                {showAssistantLiveState && (
                                    <span role="status" aria-live="polite" className="inline-flex items-center gap-1 rounded-[calc(var(--panel-radius)*0.35)] border px-1.5 py-0.5 text-[9px] font-medium text-(--accent-ai)" style={assistantLiveStyle}>
                                        <Loader2 aria-hidden="true" className="h-2.5 w-2.5 animate-spin" />
                                        {t('chatMessage.live')}
                                    </span>
                                )}
                                {isTool && message.tool_call_id && (
                                    <span className="rounded-[calc(var(--panel-radius)*0.35)] border border-(--border-subtle) bg-(--bg-app) px-1.5 py-0.5 text-[9px] font-mono text-(--fg-tertiary)">
                                        {message.tool_call_id.slice(0, 8)}
                                    </span>
                                )}
                            </div>
                            <div className="h-px w-[95%] self-center bg-(--border-default)" style={isUser ? userDividerStyle : assistantDividerStyle} />
                        </div>
                    )}

                <div className="space-y-1.5">
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
                                    <div className="overflow-hidden rounded-[calc(var(--panel-radius)*0.45)] border border-(--border-subtle) bg-(--bg-surface)/40">
                                        <img
                                            src={image.previewUrl}
                                            alt={image.name}
                                            loading="lazy"
                                            className="w-full max-h-32 object-contain"
                                        />
                                    </div>
                                    {image.name && (
                                        <div className="mt-1 text-[10px] text-(--fg-tertiary) truncate">
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

                {showInlineWorkLog && isAssistant && compactWorkEntries.length > 0 && (
                    <CompactWorkLog
                        entries={compactWorkEntries}
                        showDetails={effectiveShowDetailedWork}
                        detailsLockedOpen={detailsLockedOpen}
                        onToggleDetails={handleToggleWorkDetails}
                    />
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
                            
                            return renderSegments.map((segment, segmentIndex) => {
                                const previousSegment = segmentIndex > 0 ? renderSegments[segmentIndex - 1] : null;
                                if (segment.kind === 'activity_group') {
                                    if (!shouldShowDetailedWork) {
                                        return null;
                                    }
                                    return (
                                        <ActivityGroupDisplay
                                            key={segment.id}
                                            items={segment.items}
                                            pendingActions={pendingActions}
                                            pendingApprovalRequest={pendingApprovalRequest}
                                            onApproveCommand={onApproveCommand}
                                            onSkipCommand={onSkipCommand}
                                            onApproveApprovalRequest={onApproveApprovalRequest}
                                            onDenyApprovalRequest={onDenyApprovalRequest}
                                            onApproveSingleCommand={onApproveSingleCommand}
                                            onSkipSingleCommand={onSkipSingleCommand}
                                            onUndoTool={onUndoTool}
                                            onStopCommand={onStopCommand}
                                            onOpenFile={onOpenFile}
                                            workspaceRoot={workspaceRoot}
                                            registerActivityTarget={registerActivityTarget}
                                        />
                                    );
                                }

                                const { block, index: idx } = segment;
                                if (block.type === 'reasoning') {
                                    // Only the last reasoning block is active
                                    const isReasoningActive = isActiveReasoningBlock(block.id) || (
                                        stream?.activeBlockId === undefined
                                        && stream?.activeKind === undefined
                                        && isActive
                                        && idx === lastReasoningIdx
                                    );
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
                                    <div key={block.id || `text-${idx}`} className="select-text">
                                        {previousSegment?.kind === 'activity_group' && (
                                            <div className="mb-1.5 h-px w-full bg-(--border-default)" style={assistantDividerStyle} />
                                        )}
                                        {renderTextContent(block.content, isActiveContentBlock(block.id), isLiveContentBlock(block.id))}
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

                        {/* Legacy: Render commandExecutions that don't have block entries (backward compat) */}
                        {shouldShowDetailedWork && message.commandExecutions && message.commandExecutions.length > 0 && (
                            (() => {
                                const blockIds = new Set((message.blocks || []).filter(b => b.type === 'command_execution').map(b => b.id));
                                const orphanedCmds = message.commandExecutions.filter(c => !blockIds.has(c.id));
                                if (orphanedCmds.length === 0) return null;
                                return (
                                    <div className="mt-1.5 space-y-1.5">
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
                                    <div className="select-text">
                                        {renderTextContent(
                                            initialText,
                                            shouldUseStreamingMarkdown && stream?.activeKind === 'content' && !hasToolCalls,
                                            hasLiveTextStream && stream?.activeKind === 'content' && !hasToolCalls,
                                        )}
                                    </div>
                                )}
                                {hasToolCalls && shouldShowDetailedWork && (
                                    <div className="space-y-1.5">
                                        {toolCalls
                                            .map((call, idx) => {
                                                const matchingPendingAction = pendingActions?.find((action) => action.id === call.id);
                                                const matchingApprovalRequest = pendingApprovalRequest?.toolCallId === call.id ? pendingApprovalRequest : null;
                                                if (call.function.name === 'run_command' && matchingPendingAction && onApproveCommand && onSkipCommand) {
                                                    return (
                                                        <div
                                                            key={`${call.id}-${idx}`}
                                                            ref={(element) => registerActivityTarget?.(getApprovalActivityTargetKey(matchingPendingAction.id), element)}
                                                        >
                                                            <CommandApprovalCard
                                                                actions={[matchingPendingAction]}
                                                                onRun={onApproveCommand}
                                                                onSkip={onSkipCommand}
                                                                onRunSingle={onApproveSingleCommand}
                                                                onSkipSingle={onSkipSingleCommand}
                                                            />
                                                        </div>
                                                    );
                                                }
                                                if (matchingApprovalRequest && onApproveApprovalRequest && onDenyApprovalRequest) {
                                                    return (
                                                        <div
                                                            key={`${call.id}-${idx}`}
                                                            ref={(element) => registerActivityTarget?.(getApprovalActivityTargetKey(matchingApprovalRequest.toolCallId), element)}
                                                        >
                                                            <HookApprovalCard
                                                                request={matchingApprovalRequest}
                                                                onApprove={onApproveApprovalRequest}
                                                                onDeny={onDenyApprovalRequest}
                                                            />
                                                        </div>
                                                    );
                                                }

                                                return (
                                                    <div
                                                        key={`${call.id}-${idx}`}
                                                        ref={(element) => registerActivityTarget?.(getToolActivityTargetKey(call.id), element)}
                                                    >
                                                        <ToolCallDisplay
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
                                                            workspaceRoot={workspaceRoot}
                                                        />
                                                    </div>
                                                );
                                            })}
                                    </div>
                                )}
                                {finalText && (
                                    <div className="select-text">
                                        {hasToolCalls && <div className="mb-1.5 h-px w-full bg-(--border-default)" style={assistantDividerStyle} />}
                                        {renderTextContent(
                                            finalText,
                                            shouldUseStreamingMarkdown && stream?.activeKind === 'content',
                                            hasLiveTextStream && stream?.activeKind === 'content',
                                        )}
                                    </div>
                                )}
                                {shouldShowDetailedWork && message.commandExecutions && message.commandExecutions.length > 0 && (
                                    <div className="mt-1.5 space-y-1.5">
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

                {hasChunkCounter && (
                    <div
                        className="mt-1.5 inline-flex items-center gap-2 rounded-full border px-2.5 py-1 text-[10px] font-mono"
                        style={assistantChunkCounterStyle}
                    >
                        <span>{stream!.seq} chunks</span>
                        <span style={assistantChunkCounterDividerStyle}>•</span>
                        <span>
                            {stream!.endTime
                                ? `${streamElapsedSec.toFixed(1)}s`
                                : (showAssistantLiveState && streamAgeMs > 5000 ? 'waiting...' : 'streaming...')}
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
    if (prevProps.showInlineWorkLog !== nextProps.showInlineWorkLog) return false;
    if (prevProps.workDetailsVisible !== nextProps.workDetailsVisible) return false;
    if (prevProps.workspaceRoot !== nextProps.workspaceRoot) return false;
    
    // Message content comparison - the most important check
    const prevMsg = prevProps.message;
    const nextMsg = nextProps.message;
    if (prevMsg.id !== nextMsg.id) return false;
    if (prevMsg.content !== nextMsg.content) return false;
    if (prevMsg.reasoning !== nextMsg.reasoning) return false;
    if ((prevMsg.streaming?.seq ?? null) !== (nextMsg.streaming?.seq ?? null)) return false;
    if ((prevMsg.streaming?.endTime ?? null) !== (nextMsg.streaming?.endTime ?? null)) return false;
    if ((prevMsg.streaming?.activeKind ?? null) !== (nextMsg.streaming?.activeKind ?? null)) return false;
    if ((prevMsg.streaming?.activeBlockId ?? null) !== (nextMsg.streaming?.activeBlockId ?? null)) return false;
    const prevMentionSignature = (prevMsg.mentions || []).map((mention) => `${mention.kind}:${mention.path}:${mention.is_dir}`).join('|');
    const nextMentionSignature = (nextMsg.mentions || []).map((mention) => `${mention.kind}:${mention.path}:${mention.is_dir}`).join('|');
    if (prevMentionSignature !== nextMentionSignature) return false;
    if (prevMsg.tool_calls?.length !== nextMsg.tool_calls?.length) return false;
    if (prevMsg.commandExecutions?.length !== nextMsg.commandExecutions?.length) return false;
    if (prevMsg.blocks?.length !== nextMsg.blocks?.length) return false;
    
    // Check tool call statuses (important for showing execution state)
    if (prevMsg.tool_calls && nextMsg.tool_calls) {
        for (let i = 0; i < prevMsg.tool_calls.length; i++) {
            if (prevMsg.tool_calls[i].status !== nextMsg.tool_calls[i].status) return false;
            if (prevMsg.tool_calls[i].result !== nextMsg.tool_calls[i].result) return false;
            if (prevMsg.tool_calls[i].function.name !== nextMsg.tool_calls[i].function.name) return false;
            if (prevMsg.tool_calls[i].function.arguments !== nextMsg.tool_calls[i].function.arguments) return false;
        }
    }
    if (prevMsg.commandExecutions && nextMsg.commandExecutions) {
        for (let i = 0; i < prevMsg.commandExecutions.length; i++) {
            if (prevMsg.commandExecutions[i].command !== nextMsg.commandExecutions[i].command) return false;
            if (prevMsg.commandExecutions[i].exitCode !== nextMsg.commandExecutions[i].exitCode) return false;
            if (prevMsg.commandExecutions[i].output !== nextMsg.commandExecutions[i].output) return false;
        }
    }
    
    // Pending actions - check both reference AND presence change
    const prevHasPending = prevProps.pendingActions && prevProps.pendingActions.length > 0;
    const nextHasPending = nextProps.pendingActions && nextProps.pendingActions.length > 0;
    if (prevHasPending !== nextHasPending) return false;
    if (prevProps.pendingActions !== nextProps.pendingActions) return false;
    if (prevProps.pendingApprovalRequest?.toolCallId !== nextProps.pendingApprovalRequest?.toolCallId) return false;
    
    // Callbacks - check presence change (undefined vs function)
    const prevHasApprove = !!prevProps.onApproveCommand;
    const nextHasApprove = !!nextProps.onApproveCommand;
    const prevHasSkip = !!prevProps.onSkipCommand;
    const nextHasSkip = !!nextProps.onSkipCommand;
    if (prevHasApprove !== nextHasApprove) return false;
    if (prevHasSkip !== nextHasSkip) return false;
    
    return true;
});
