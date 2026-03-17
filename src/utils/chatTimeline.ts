import type { ChatMessage as ChatMessageType, CommandExecution, MessageBlock, ToolCall } from '../types/chat';
import type { StructuredAction } from '../types/events';

const ALWAYS_UNVIRTUALIZED_TAIL_ROWS = 10;

export interface DerivedChatMessageRow {
    kind: 'message';
    key: string;
    message: ChatMessageType;
    isContinued: boolean;
    isActive: boolean;
    pendingActions?: StructuredAction[];
}

export type DerivedChatRow = DerivedChatMessageRow;

export type DerivedActivityGroupItem =
    | { kind: 'tool_call'; id: string; toolCall: ToolCall }
    | { kind: 'command_execution'; id: string; commandExecution: CommandExecution };

export type DerivedRenderSegment =
    | { kind: 'block'; block: MessageBlock; index: number }
    | { kind: 'activity_group'; id: string; items: DerivedActivityGroupItem[] };

export interface ChatRowHeightLayout {
    viewportWidthPx?: number | null;
}

function clampPositive(value: number, fallback: number): number {
    return Number.isFinite(value) && value > 0 ? value : fallback;
}

function estimateWrappedLineCount(text: string, charactersPerLine: number): number {
    if (!text) {
        return 0;
    }

    const normalizedCharsPerLine = Math.max(12, Math.floor(charactersPerLine));
    return text
        .split('\n')
        .reduce((total, line) => total + Math.max(1, Math.ceil(line.length / normalizedCharsPerLine)), 0);
}

function estimateMessageBodyHeight(message: ChatMessageType, viewportWidthPx?: number | null): number {
    const width = clampPositive(viewportWidthPx ?? 0, 820);
    const textCharsPerLine = Math.max(24, Math.floor((width - 132) / 8.2));
    const cardBase = 74;
    const contentText = message.content_after_tools || message.content_before_tools || message.content || '';
    const textLines = estimateWrappedLineCount(contentText, textCharsPerLine);
    const reasoningLines = estimateWrappedLineCount(message.reasoning || '', textCharsPerLine - 4);
    const textHeight = textLines > 0 ? Math.max(28, textLines * 21) : 0;
    const reasoningHeight = reasoningLines > 0 ? Math.min(180, 32 + reasoningLines * 14) : 0;
    const imageCount = message.images?.length ?? 0;
    const imageRows = imageCount === 0 ? 0 : Math.ceil(imageCount / 2);
    const imageHeight = imageRows * 170;
    const toolRows = (message.tool_calls?.length ?? 0) + (message.commandExecutions?.length ?? 0);
    const toolHeight = toolRows > 0 ? toolRows * 72 : 0;
    const mentionHeight = message.mentions && message.mentions.length > 0 ? 52 : 0;
    const planHeight = message.planSummary?.todos?.length ? 56 : 0;
    return cardBase + textHeight + reasoningHeight + imageHeight + toolHeight + mentionHeight + planHeight;
}

export function estimateChatRowHeight(
    row: DerivedChatRow,
    layout: ChatRowHeightLayout = {},
): number {
    const message = row.message;
    const pendingApprovalHeight = row.pendingActions && row.pendingActions.length > 0 ? 128 : 0;
    const continuedAdjustment = row.isContinued ? -16 : 0;

    if (message.role === 'User') {
        return Math.max(88, estimateMessageBodyHeight(message, layout.viewportWidthPx) + pendingApprovalHeight + continuedAdjustment);
    }

    if (message.role === 'Assistant') {
        return Math.max(96, estimateMessageBodyHeight(message, layout.viewportWidthPx) + pendingApprovalHeight + continuedAdjustment + 8);
    }

    return Math.max(80, estimateMessageBodyHeight(message, layout.viewportWidthPx) + pendingApprovalHeight + continuedAdjustment);
}

export function findFirstUnvirtualizedChatRowIndex(
    rows: DerivedChatRow[],
    loading: boolean,
): number {
    const firstTailIndex = Math.max(rows.length - ALWAYS_UNVIRTUALIZED_TAIL_ROWS, 0);
    if (!loading) {
        return firstTailIndex;
    }

    const activeIndex = rows.findIndex((row) => row.isActive);
    if (activeIndex < 0) {
        return firstTailIndex;
    }

    for (let index = activeIndex - 1; index >= 0; index -= 1) {
        const row = rows[index];
        if (row?.message.role === 'User') {
            return Math.min(index, firstTailIndex);
        }
        if (row?.message.role === 'Assistant' && !row.isActive) {
            break;
        }
    }

    return Math.min(activeIndex, firstTailIndex);
}

function findPendingActionTargetIndex(
    messages: ChatMessageType[],
    pendingActions: StructuredAction[] | null,
): number {
    if (!pendingActions || pendingActions.length === 0) {
        return -1;
    }

    const pendingIds = new Set(pendingActions.map((action) => action.id));
    for (let index = messages.length - 1; index >= 0; index -= 1) {
        const message = messages[index];
        if (message?.role !== 'Assistant') {
            continue;
        }
        if (message.tool_calls?.some((toolCall) => pendingIds.has(toolCall.id))) {
            return index;
        }
    }

    for (let index = messages.length - 1; index >= 0; index -= 1) {
        if (messages[index]?.role === 'Assistant') {
            return index;
        }
    }

    return -1;
}

export function deriveMessageRenderSegments(
    message: ChatMessageType,
    pendingActions?: StructuredAction[] | null,
): DerivedRenderSegment[] {
    if (!message.blocks || message.blocks.length === 0) {
        return [];
    }

    const toolCallById = new Map((message.tool_calls || []).map((toolCall) => [toolCall.id, toolCall]));
    const commandExecutionById = new Map((message.commandExecutions || []).map((execution) => [execution.id, execution]));

    const segments: DerivedRenderSegment[] = [];
    let index = 0;

    while (index < message.blocks.length) {
        const block = message.blocks[index];
        if (block.type !== 'tool_call' && block.type !== 'command_execution') {
            segments.push({ kind: 'block', block, index });
            index += 1;
            continue;
        }

        const items: DerivedActivityGroupItem[] = [];
        let cursor = index;

        while (cursor < message.blocks.length) {
            const candidate = message.blocks[cursor];

            if (candidate.type === 'tool_call') {
                const toolCall = toolCallById.get(candidate.id);
                if (toolCall) {
                    items.push({ kind: 'tool_call', id: candidate.id, toolCall });
                }
                cursor += 1;
                continue;
            }

            if (candidate.type === 'command_execution') {
                const commandExecution = commandExecutionById.get(candidate.id);
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
}

export function deriveChatRows(
    messages: ChatMessageType[],
    loading: boolean,
    pendingActions: StructuredAction[] | null,
): DerivedChatRow[] {
    const pendingActionTargetIndex = findPendingActionTargetIndex(messages, pendingActions);

    return messages.map((message, index) => {
        const isLast = index === messages.length - 1;
        const isAssistant = message.role === 'Assistant';
        const prevMessage = index > 0 ? messages[index - 1] : null;
        const isContinued = isAssistant && prevMessage?.role === 'Assistant';
        const showPendingActions = index === pendingActionTargetIndex && !!pendingActions && pendingActions.length > 0;

        return {
            kind: 'message',
            key: message.id || `${message.role}-${index}`,
            message,
            isContinued,
            isActive: isLast && loading,
            ...(showPendingActions ? { pendingActions } : {}),
        };
    });
}
