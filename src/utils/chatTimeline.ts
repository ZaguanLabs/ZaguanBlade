import type { ChatMessage as ChatMessageType, CommandExecution, HookApprovalRequest, MessageBlock, ToolCall } from '../types/chat';
import type { StructuredAction } from '../types/events';

const ALWAYS_UNVIRTUALIZED_TAIL_ROWS = 10;

export interface DerivedChatMessageRow {
    kind: 'message';
    key: string;
    message: ChatMessageType;
    isContinued: boolean;
    isActive: boolean;
    pendingActions?: StructuredAction[];
    pendingApprovalRequest?: HookApprovalRequest;
}

export type DerivedChatRow = DerivedChatMessageRow;

export interface DerivedChatWorkLogRow {
    kind: 'work_log';
    key: string;
    message: ChatMessageType;
    entries: ChatWorkEntry[];
    pendingActions?: StructuredAction[];
    pendingApprovalRequest?: HookApprovalRequest;
}

export type DerivedChatTimelineRow = DerivedChatMessageRow | DerivedChatWorkLogRow;

export type DerivedActivityGroupItem =
    | { kind: 'tool_call'; id: string; toolCall: ToolCall }
    | { kind: 'command_execution'; id: string; commandExecution: CommandExecution };

export type DerivedRenderSegment =
    | { kind: 'block'; block: MessageBlock; index: number }
    | { kind: 'activity_group'; id: string; items: DerivedActivityGroupItem[] };

export interface ChatWorkEntry {
    id: string;
    messageId: string;
    source: 'tool_call' | 'command_execution' | 'activity';
    label: string;
    tone: 'tool' | 'error' | 'info';
    detail?: string;
    command?: string;
    toolCallId?: string;
    commandExecutionId?: string;
    status?: ToolCall['status'];
}

export interface ChatActivity {
    id: string;
    kind: 'tool';
    toolName: string;
    action: string;
    messageId?: string;
    toolCallId?: string;
    filePath?: string;
    detail?: string;
    command?: string;
    status?: ToolCall['status'];
    startedAt?: number;
    updatedAt?: number;
}

export interface ChatProjection {
    messages: ChatMessageType[];
    messageById: Map<string, ChatMessageType>;
    activities: ChatActivity[];
    activityById: Map<string, ChatActivity>;
    workEntries: ChatWorkEntry[];
    workEntriesByMessageId: Map<string, ChatWorkEntry[]>;
}

export interface ChatActiveWorkState {
    activeEntries: ChatWorkEntry[];
    activeEntriesByMessageId: Map<string, ChatWorkEntry[]>;
    activeMessageIds: Set<string>;
}

export interface StableChatRowsState {
    byKey: Map<string, DerivedChatRow>;
    rows: DerivedChatRow[];
}

export interface StableChatTimelineRowsState {
    byKey: Map<string, DerivedChatTimelineRow>;
    rows: DerivedChatTimelineRow[];
}

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
    const pendingApprovalHeight = (row.pendingActions && row.pendingActions.length > 0) || row.pendingApprovalRequest ? 128 : 0;
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

function findPendingApprovalTargetIndex(
    messages: ChatMessageType[],
    pendingApprovalRequest: HookApprovalRequest | null | undefined,
): number {
    if (!pendingApprovalRequest) {
        return -1;
    }

    for (let index = messages.length - 1; index >= 0; index -= 1) {
        const message = messages[index];
        if (message?.role !== 'Assistant') {
            continue;
        }
        if (message.tool_calls?.some((toolCall) => toolCall.id === pendingApprovalRequest.toolCallId)) {
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

function parseToolArguments(value: string): Record<string, unknown> | null {
    try {
        const parsed = JSON.parse(value);
        return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
            ? parsed as Record<string, unknown>
            : null;
    } catch {
        return null;
    }
}

function humanizeToolName(value: string): string {
    const words = value
        .replace(/[_-]+/g, ' ')
        .trim()
        .split(/\s+/)
        .filter(Boolean);
    if (words.length === 0) {
        return 'Tool call';
    }
    return words.map((word) => `${word.charAt(0).toUpperCase()}${word.slice(1)}`).join(' ');
}

function firstStringField(record: Record<string, unknown> | null, fields: string[]): string | undefined {
    if (!record) {
        return undefined;
    }
    for (const field of fields) {
        const value = record[field];
        if (typeof value === 'string' && value.trim().length > 0) {
            return value.trim();
        }
    }
    return undefined;
}

function deriveToolCallWorkEntry(messageId: string, toolCall: ToolCall): ChatWorkEntry {
    const name = toolCall.function.name;
    const args = parseToolArguments(toolCall.function.arguments);
    const command = name === 'run_command'
        ? firstStringField(args, ['command', 'cmd'])
        : undefined;
    const detail = command
        ?? firstStringField(args, ['path', 'file_path', 'relative_path', 'query', 'pattern'])
        ?? (toolCall.result && toolCall.result.trim().length > 0 ? toolCall.result.trim().split(/\r?\n/, 1)[0] : undefined);
    const tone = toolCall.status === 'error' || toolCall.status === 'skipped' ? 'error' : 'tool';

    return {
        id: `tool:${toolCall.id}`,
        messageId,
        source: 'tool_call',
        label: command ? 'Run command' : humanizeToolName(name),
        tone,
        ...(detail ? { detail } : {}),
        ...(command ? { command } : {}),
        toolCallId: toolCall.id,
        status: toolCall.status,
    };
}

function deriveCommandExecutionWorkEntry(messageId: string, execution: CommandExecution): ChatWorkEntry {
    const tone = execution.exitCode === 0 ? 'tool' : 'error';
    return {
        id: `command:${execution.id}`,
        messageId,
        source: 'command_execution',
        label: 'Ran command',
        tone,
        detail: execution.command,
        command: execution.command,
        commandExecutionId: execution.id,
    };
}

function findActivityMessageId(activity: ChatActivity, messages: ChatMessageType[]): string | null {
    if (activity.messageId) {
        return activity.messageId;
    }
    if (activity.toolCallId) {
        const owner = messages.find((message) => message.tool_calls?.some((toolCall) => toolCall.id === activity.toolCallId));
        if (owner?.id) {
            return owner.id;
        }
    }
    const lastAssistant = [...messages].reverse().find((message) => message.role === 'Assistant' && message.id);
    return lastAssistant?.id ?? null;
}

function deriveActivityWorkEntry(activity: ChatActivity, messageId: string): ChatWorkEntry {
    const command = activity.command;
    const detail = command ?? activity.detail ?? activity.filePath ?? activity.action;
    const tone = activity.status === 'error' || activity.status === 'skipped' ? 'error' : 'info';
    return {
        id: `activity:${activity.id}`,
        messageId,
        source: 'activity',
        label: command ? 'Run command' : humanizeToolName(activity.toolName),
        tone,
        ...(detail ? { detail } : {}),
        ...(command ? { command } : {}),
        ...(activity.toolCallId ? { toolCallId: activity.toolCallId } : {}),
        ...(activity.status ? { status: activity.status } : {}),
    };
}

export function deriveChatWorkEntries(messages: ChatMessageType[]): ChatWorkEntry[] {
    const entries: ChatWorkEntry[] = [];
    for (const message of messages) {
        if (message.role !== 'Assistant' || !message.id) {
            continue;
        }

        const segments = deriveMessageRenderSegments(message);
        const hasOrderedActivitySegments = segments.some((segment) => segment.kind === 'activity_group');
        if (hasOrderedActivitySegments) {
            for (const segment of segments) {
                if (segment.kind !== 'activity_group') {
                    continue;
                }
                for (const item of segment.items) {
                    if (item.kind === 'tool_call') {
                        entries.push(deriveToolCallWorkEntry(message.id, item.toolCall));
                    } else {
                        entries.push(deriveCommandExecutionWorkEntry(message.id, item.commandExecution));
                    }
                }
            }
            continue;
        }

        for (const toolCall of message.tool_calls || []) {
            entries.push(deriveToolCallWorkEntry(message.id, toolCall));
        }
        for (const execution of message.commandExecutions || []) {
            entries.push(deriveCommandExecutionWorkEntry(message.id, execution));
        }
    }
    return entries;
}

function hasEquivalentToolWorkEntry(entries: ChatWorkEntry[], entry: ChatWorkEntry): boolean {
    return !!entry.toolCallId && entries.some((candidate) =>
        candidate.toolCallId === entry.toolCallId
        && candidate.source !== 'activity'
    );
}

export function deriveChatProjection(messages: ChatMessageType[], activities: ChatActivity[] = []): ChatProjection {
    const messageById = new Map<string, ChatMessageType>();
    const activityById = new Map<string, ChatActivity>();
    const workEntriesByMessageId = new Map<string, ChatWorkEntry[]>();
    const workEntries: ChatWorkEntry[] = [];

    messages.forEach((message, index) => {
        const messageId = message.id || `${message.role}-${index}`;
        messageById.set(messageId, message);
        const messageWorkEntries = deriveChatWorkEntries([message]);
        if (messageWorkEntries.length > 0) {
            workEntriesByMessageId.set(messageId, messageWorkEntries);
            workEntries.push(...messageWorkEntries);
        }
    });

    for (const activity of activities) {
        activityById.set(activity.id, activity);
        const messageId = findActivityMessageId(activity, messages);
        if (!messageId) {
            continue;
        }
        const activityEntry = deriveActivityWorkEntry(activity, messageId);
        const existingEntries = workEntriesByMessageId.get(messageId) ?? [];
        if (hasEquivalentToolWorkEntry(existingEntries, activityEntry)) {
            continue;
        }
        const nextEntries = [...existingEntries, activityEntry];
        workEntriesByMessageId.set(messageId, nextEntries);
        workEntries.push(activityEntry);
    }

    return {
        messages,
        messageById,
        activities,
        activityById,
        workEntries,
        workEntriesByMessageId,
    };
}

export function stabilizeChatProjection(projection: ChatProjection, previous: ChatProjection | null | undefined): ChatProjection {
    if (!previous) {
        return projection;
    }

    const previousWorkEntryById = new Map(previous.workEntries.map((entry) => [entry.id, entry]));
    let workEntriesChanged = projection.workEntries.length !== previous.workEntries.length;
    const workEntries = projection.workEntries.map((entry, index) => {
        const previousEntry = previousWorkEntryById.get(entry.id);
        const nextEntry = previousEntry && areWorkEntryEqual(previousEntry, entry) ? previousEntry : entry;
        if (!workEntriesChanged && previous.workEntries[index] !== nextEntry) {
            workEntriesChanged = true;
        }
        return nextEntry;
    });

    const stableWorkEntryById = new Map(workEntries.map((entry) => [entry.id, entry]));
    let workEntriesByMessageIdChanged = projection.workEntriesByMessageId.size !== previous.workEntriesByMessageId.size;
    const workEntriesByMessageId = new Map<string, ChatWorkEntry[]>();
    for (const [messageId, entries] of projection.workEntriesByMessageId) {
        const stableEntries = entries.map((entry) => stableWorkEntryById.get(entry.id) ?? entry);
        const previousEntries = previous.workEntriesByMessageId.get(messageId);
        const nextEntries = previousEntries && areWorkEntriesEqual(previousEntries, stableEntries)
            ? previousEntries
            : stableEntries;
        workEntriesByMessageId.set(messageId, nextEntries);
        if (!workEntriesByMessageIdChanged && previousEntries !== nextEntries) {
            workEntriesByMessageIdChanged = true;
        }
    }

    if (
        projection.messages === previous.messages
        && projection.activities === previous.activities
        && !workEntriesChanged
        && !workEntriesByMessageIdChanged
    ) {
        return previous;
    }

    return {
        ...projection,
        workEntries: workEntriesChanged ? workEntries : previous.workEntries,
        workEntriesByMessageId: workEntriesByMessageIdChanged ? workEntriesByMessageId : previous.workEntriesByMessageId,
    };
}

function isActiveWorkEntry(entry: ChatWorkEntry): boolean {
    return entry.status === 'executing' || entry.status === 'pending';
}

export function deriveChatActiveWorkState(projection: ChatProjection): ChatActiveWorkState {
    const activeEntries = projection.workEntries.filter(isActiveWorkEntry);
    const activeEntriesByMessageId = new Map<string, ChatWorkEntry[]>();
    const activeMessageIds = new Set<string>();

    for (const entry of activeEntries) {
        activeMessageIds.add(entry.messageId);
        const existingEntries = activeEntriesByMessageId.get(entry.messageId);
        if (existingEntries) {
            existingEntries.push(entry);
        } else {
            activeEntriesByMessageId.set(entry.messageId, [entry]);
        }
    }

    return {
        activeEntries,
        activeEntriesByMessageId,
        activeMessageIds,
    };
}

export function deriveChatRows(
    messages: ChatMessageType[],
    loading: boolean,
    pendingActions: StructuredAction[] | null,
    pendingApprovalRequest?: HookApprovalRequest | null,
): DerivedChatRow[] {
    const pendingActionTargetIndex = findPendingActionTargetIndex(messages, pendingActions);
    const pendingApprovalTargetIndex = findPendingApprovalTargetIndex(messages, pendingApprovalRequest);

    return messages.map((message, index) => {
        const isLast = index === messages.length - 1;
        const isAssistant = message.role === 'Assistant';
        const prevMessage = index > 0 ? messages[index - 1] : null;
        const isContinued = isAssistant && prevMessage?.role === 'Assistant';
        const showPendingActions = index === pendingActionTargetIndex && !!pendingActions && pendingActions.length > 0;
        const showPendingApprovalRequest = index === pendingApprovalTargetIndex && !!pendingApprovalRequest;

        return {
            kind: 'message',
            key: message.id || `${message.role}-${index}`,
            message,
            isContinued,
            isActive: isLast && loading,
            ...(showPendingActions ? { pendingActions } : {}),
            ...(showPendingApprovalRequest ? { pendingApprovalRequest } : {}),
        };
    });
}

export function deriveChatTimelineRowsFromProjection(
    projection: ChatProjection,
    loading: boolean,
    pendingActions: StructuredAction[] | null,
    pendingApprovalRequest?: HookApprovalRequest | null,
): DerivedChatTimelineRow[] {
    const messageRows = deriveChatRows(projection.messages, loading, pendingActions, pendingApprovalRequest);
    const timelineRows: DerivedChatTimelineRow[] = [];
    for (const row of messageRows) {
        timelineRows.push(row);
        if (row.message.role !== 'Assistant') {
            continue;
        }
        const entries = projection.workEntriesByMessageId.get(row.message.id || row.key) ?? [];
        if (entries.length === 0) {
            continue;
        }
        timelineRows.push({
            kind: 'work_log',
            key: `${row.key}:work-log`,
            message: row.message,
            entries,
            ...(row.pendingActions ? { pendingActions: row.pendingActions } : {}),
            ...(row.pendingApprovalRequest ? { pendingApprovalRequest: row.pendingApprovalRequest } : {}),
        });
    }
    return timelineRows;
}

export function deriveChatTimelineRows(
    messages: ChatMessageType[],
    loading: boolean,
    pendingActions: StructuredAction[] | null,
    pendingApprovalRequest?: HookApprovalRequest | null,
    activities: ChatActivity[] = [],
): DerivedChatTimelineRow[] {
    return deriveChatTimelineRowsFromProjection(
        deriveChatProjection(messages, activities),
        loading,
        pendingActions,
        pendingApprovalRequest,
    );
}

function areStructuredActionsEqual(
    left: StructuredAction[] | undefined,
    right: StructuredAction[] | undefined,
): boolean {
    if (left === right) {
        return true;
    }
    if (!left || !right || left.length !== right.length) {
        return false;
    }
    return left.every((action, index) => action === right[index]);
}

function areRowsEqual(left: DerivedChatRow, right: DerivedChatRow): boolean {
    return left.kind === right.kind
        && left.key === right.key
        && left.message === right.message
        && left.isContinued === right.isContinued
        && left.isActive === right.isActive
        && left.pendingApprovalRequest === right.pendingApprovalRequest
        && areStructuredActionsEqual(left.pendingActions, right.pendingActions);
}

function areWorkEntryEqual(left: ChatWorkEntry, right: ChatWorkEntry): boolean {
    return left.id === right.id
        && left.messageId === right.messageId
        && left.source === right.source
        && left.label === right.label
        && left.tone === right.tone
        && left.detail === right.detail
        && left.command === right.command
        && left.toolCallId === right.toolCallId
        && left.commandExecutionId === right.commandExecutionId
        && left.status === right.status;
}

function areWorkEntriesEqual(left: ChatWorkEntry[], right: ChatWorkEntry[]): boolean {
    if (left === right) {
        return true;
    }
    if (left.length !== right.length) {
        return false;
    }
    return left.every((entry, index) => {
        const other = right[index];
        return !!other && areWorkEntryEqual(entry, other);
    });
}

function areTimelineRowsEqual(
    left: DerivedChatTimelineRow,
    right: DerivedChatTimelineRow,
): boolean {
    if (left.kind !== right.kind || left.key !== right.key || left.message !== right.message) {
        return false;
    }
    if (left.kind === 'message' && right.kind === 'message') {
        return areRowsEqual(left, right);
    }
    if (left.kind === 'work_log' && right.kind === 'work_log') {
        return left.pendingApprovalRequest === right.pendingApprovalRequest
            && areStructuredActionsEqual(left.pendingActions, right.pendingActions)
            && areWorkEntriesEqual(left.entries, right.entries);
    }
    return false;
}

export function computeStableChatRows(
    rows: DerivedChatRow[],
    previous: StableChatRowsState,
): StableChatRowsState {
    const nextByKey = new Map<string, DerivedChatRow>();
    let changed = rows.length !== previous.rows.length;
    const stableRows = rows.map((row, index) => {
        const previousRow = previous.byKey.get(row.key);
        const nextRow = previousRow && areRowsEqual(previousRow, row) ? previousRow : row;
        nextByKey.set(row.key, nextRow);
        if (!changed && previous.rows[index] !== nextRow) {
            changed = true;
        }
        return nextRow;
    });

    return changed ? { byKey: nextByKey, rows: stableRows } : previous;
}

export function computeStableChatTimelineRows(
    rows: DerivedChatTimelineRow[],
    previous: StableChatTimelineRowsState,
): StableChatTimelineRowsState {
    const nextByKey = new Map<string, DerivedChatTimelineRow>();
    let changed = rows.length !== previous.rows.length;
    const stableRows = rows.map((row, index) => {
        const previousRow = previous.byKey.get(row.key);
        const nextRow = previousRow && areTimelineRowsEqual(previousRow, row) ? previousRow : row;
        nextByKey.set(row.key, nextRow);
        if (!changed && previous.rows[index] !== nextRow) {
            changed = true;
        }
        return nextRow;
    });

    return changed ? { byKey: nextByKey, rows: stableRows } : previous;
}
