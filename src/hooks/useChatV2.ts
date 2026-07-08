import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import { subscribeBladeEventType, waitForBladeEvent } from '../services/bladeEvents';
import { EditorFacade, isBackendAuthoritative } from '../services/editorFacade';
import { ensureProblemCaptureStarted, getActiveDiagnostics } from '../services/problemStore';
import { useEditorActions } from '../contexts/EditorContext';
import { MessageBuffer } from '../utils/eventBuffer';
import { ensureMessagesHaveBlocks, insertAssistantMessageAfterLastUser, insertToolCallBlockPreservingOrder, moveExistingContentAfterTools, upsertSplitTextBlocks } from '../utils/messageBlocks';
import { EventNames, type ApprovalRequestPayload, type ChatDonePayload, type ChatErrorPayload, type ContextLengthExceededPayload, type MessageTooLargePayload, type RequestConfirmationPayload, type StructuredAction, type ToolExecutionCompletedPayload, type TodoItem } from '../types/events';
import type { ChatMention } from '../types/blade';
import type { ChatImage, ChatMessage, ChatMode, CognitiveInterruptState, ComposerMention, CommandExecution, HookApprovalRequest, ImageAttachment, MessageBlock, ModelInfo, QueuedRequest, StreamingState, ToolActivityState, ToolCall } from '../types/chat';
import type { ChatActivity } from '../utils/chatTimeline';
import { buildContextLengthSystemMessage, buildMessageTooLargeSystemMessage, formatChatErrorPayload } from '../utils/localizedEvents';
import i18n from '../i18n';

const TOOL_ACTIVITY_DISPATCH_INTERVAL_MS = 120;
const MESSAGE_COMPLETION_GRACE_MS = 1200;
const MAX_CHAT_ACTIVITY_HISTORY = 200;
const STREAM_DEBUG_ENABLED = false;
const MODEL_CACHE_STORAGE_KEY = 'zblade.chat.models.v1';
const COGNITIVE_INTERRUPT_CLEAR_DELAY_MS = 2500;

interface UseChatV2Options {
    autoApproveRunCommands?: boolean;
    /**
     * Returns the active file's editor-buffer content when it has unsaved
     * changes, or null when clean/unavailable. Sent with each chat turn so the
     * backend can hash the active file from the same buffer warmup used
     * (buffer-over-disk), letting zcoderd trust the warmed snapshot without a
     * redundant read.
     */
    getActiveBufferContent?: () => string | null;
}

function pickDefaultModel(models: ModelInfo[]): ModelInfo | null {
    if (models.length === 0) {
        return null;
    }

    return models.find((model) => model.id === 'anthropic/claude-sonnet-5')
        || models.find((model) => model.id === 'anthropic/claude-sonnet-4-5-20250929')
        || models.find((model) => model.id === 'openai/gpt-5.2')
        || models[0];
}

function readCachedModels(): ModelInfo[] {
    if (typeof window === 'undefined') {
        return [];
    }

    try {
        const raw = window.localStorage.getItem(MODEL_CACHE_STORAGE_KEY);
        if (!raw) {
            return [];
        }

        const parsed = JSON.parse(raw);
        if (!Array.isArray(parsed)) {
            return [];
        }

        return parsed.filter((model): model is ModelInfo => {
            return typeof model?.id === 'string'
                && typeof model?.name === 'string'
                && typeof model?.description === 'string';
        });
    } catch {
        return [];
    }
}

function writeCachedModels(models: ModelInfo[]): void {
    if (typeof window === 'undefined') {
        return;
    }

    try {
        window.localStorage.setItem(MODEL_CACHE_STORAGE_KEY, JSON.stringify(models));
    } catch {
        // Ignore storage failures; cache is an optimization only.
    }
}

function isImageAttachment(image: ChatImage): image is ImageAttachment {
    const candidate = image as Partial<ImageAttachment>;
    return typeof candidate.id === 'string'
        && typeof candidate.dataUrl === 'string'
        && typeof candidate.thumbnailUrl === 'string';
}

function areToolActivitiesEqual(a: ToolActivityState | null, b: ToolActivityState | null): boolean {
    if (a === b) {
        return true;
    }
    if (!a || !b) {
        return false;
    }
    return a.toolName === b.toolName
        && a.filePath === b.filePath
        && a.action === b.action
        && a.toolCallId === b.toolCallId
        && a.chunkCount === b.chunkCount
        && a.startedAt === b.startedAt
        && a.lastChunkAt === b.lastChunkAt;
}

function isWhitespaceOnly(value: string): boolean {
    return value.trim().length === 0;
}

function findLastBlockIdOfType(blocks: MessageBlock[], type: 'text' | 'reasoning'): string | undefined {
    for (let index = blocks.length - 1; index >= 0; index -= 1) {
        const block = blocks[index];
        if (block.type === type && !isWhitespaceOnly(block.content)) {
            return block.id;
        }
    }

    return undefined;
}

function markStreamingToolActivity(streaming: StreamingState | undefined): StreamingState | undefined {
    if (!streaming) {
        return undefined;
    }

    return {
        ...streaming,
        activeKind: 'tool',
        activeBlockId: undefined,
    };
}

function updateStreamingStateMap(
    states: Map<string, StreamingState>,
    messageId: string,
    streaming: StreamingState | undefined,
): StreamingState | undefined {
    if (streaming) {
        states.set(messageId, streaming);
    }

    return streaming;
}

function isRunCommandAction(action: StructuredAction): boolean {
    return action.actionKind === 'command' || action.is_generic_tool === false;
}

function streamDebugPreview(value: string): string {
    const normalized = value.replace(/\n/g, '\\n').replace(/\r/g, '\\r');
    return normalized.length > 160 ? `${normalized.slice(0, 160)}…` : normalized;
}

function streamDebugLog(tag: string, payload: Record<string, unknown>): void {
    const message = `${tag} ${JSON.stringify(payload)}`;
    console.debug(message);
    void invoke('log_frontend', { message }).catch(() => undefined);
}

function cognitiveInterruptStatus(state: string): ToolCall['status'] {
    if (state === 'cleared') {
        return 'complete';
    }
    return 'executing';
}

function cognitiveInterruptActivityDetail(interrupt: CognitiveInterruptState): string {
    if (interrupt.state === 'blocked' && interrupt.tool_name) {
        return i18n.t('chat.cognitiveInterrupt.toolPaused', { toolName: interrupt.tool_name, defaultValue: '{{toolName}} paused until diagnostic evidence is gathered' });
    }
    return interrupt.summary;
}

function findTextBeforeFirstActivity(blocks: MessageBlock[]): string | undefined {
    const firstActivityIndex = blocks.findIndex((block) => block.type === 'tool_call' || block.type === 'command_execution');
    if (firstActivityIndex === -1) {
        return undefined;
    }

    for (let index = firstActivityIndex - 1; index >= 0; index -= 1) {
        const block = blocks[index];
        if (block.type === 'text') {
            return block.content;
        }
    }

    return undefined;
}

function findTextAfterLastActivity(blocks: MessageBlock[]): string | undefined {
    const lastActivityIndex = blocks.reduce((lastIndex, block, index) => {
        return block.type === 'tool_call' || block.type === 'command_execution' ? index : lastIndex;
    }, -1);
    if (lastActivityIndex === -1) {
        return undefined;
    }

    for (let index = lastActivityIndex + 1; index < blocks.length; index += 1) {
        const block = blocks[index];
        if (block.type === 'text') {
            return block.content;
        }
    }

    return undefined;
}

function hasInterleavedContentBetweenActivities(blocks: MessageBlock[]): boolean {
    const firstActivityIndex = blocks.findIndex((block) => block.type === 'tool_call' || block.type === 'command_execution');
    if (firstActivityIndex === -1) {
        return false;
    }

    const lastActivityIndex = blocks.reduce((lastIndex, block, index) => {
        return block.type === 'tool_call' || block.type === 'command_execution' ? index : lastIndex;
    }, -1);
    if (lastActivityIndex <= firstActivityIndex) {
        return false;
    }

    for (let index = firstActivityIndex + 1; index < lastActivityIndex; index += 1) {
        const block = blocks[index];
        if (block.type !== 'tool_call' && block.type !== 'command_execution') {
            return true;
        }
    }

    return false;
}

function hasReasoningContext(message: ChatMessage | undefined, blocks: MessageBlock[]): boolean {
    if (blocks.some((block) => block.type === 'reasoning')) {
        return true;
    }

    if (message?.reasoning && !isWhitespaceOnly(message.reasoning)) {
        return true;
    }

    return (message?.blocks || []).some((block) => block.type === 'reasoning');
}

function inferContentBeforeTools(message: ChatMessage | undefined, blocks: MessageBlock[], fullContent: string): string | undefined {
    if (message?.content_before_tools !== undefined) {
        if (message.content_before_tools.length > 0) {
            return message.content_before_tools;
        }

        const textAfterLastActivity = findTextAfterLastActivity(blocks);
        const hasOnlyActivityOrWhitespaceBlocks = blocks.every((block) => {
            return block.type === 'tool_call'
                || block.type === 'command_execution'
                || (block.type === 'text' && isWhitespaceOnly(block.content));
        });
        const hasInFlightActivity = (message.tool_calls || []).some((toolCall) => {
            return toolCall.status === undefined || toolCall.status === 'pending' || toolCall.status === 'executing';
        });
        const hasPriorReasoningContext = hasReasoningContext(message, blocks);
        if (
            !textAfterLastActivity
            && hasOnlyActivityOrWhitespaceBlocks
            && hasInFlightActivity
            && !hasPriorReasoningContext
            && !isWhitespaceOnly(fullContent)
        ) {
            return fullContent;
        }

        return message.content_before_tools;
    }

    const textBeforeFirstActivity = findTextBeforeFirstActivity(blocks);
    if (textBeforeFirstActivity !== undefined) {
        return textBeforeFirstActivity;
    }

    const firstActivityIndex = blocks.findIndex((block) => block.type === 'tool_call' || block.type === 'command_execution');
    if (firstActivityIndex === -1) {
        return undefined;
    }

    return '';
}

export function normalizeSplitBlocks(
    message: ChatMessage | undefined,
    blocks: MessageBlock[],
    fullContent: string,
): { blocks: MessageBlock[]; contentBeforeTools?: string; contentAfterTools?: string } {
    const hasActivityBlocks = blocks.some((block) => block.type === 'tool_call' || block.type === 'command_execution');
    const hasExplicitSplit = message?.content_before_tools !== undefined || message?.content_after_tools !== undefined;
    if (!hasActivityBlocks && !hasExplicitSplit) {
        return { blocks };
    }

    if (hasInterleavedContentBetweenActivities(blocks)) {
        return { blocks };
    }

    const contentBeforeTools = inferContentBeforeTools(message, blocks, fullContent) ?? '';
    const textAfterLastActivity = findTextAfterLastActivity(blocks);
    const contentAfterTools = fullContent.startsWith(contentBeforeTools)
        ? fullContent.slice(contentBeforeTools.length)
        : (textAfterLastActivity ?? message?.content_after_tools ?? fullContent);

    return {
        blocks: upsertSplitTextBlocks(blocks, contentBeforeTools, contentAfterTools),
        contentBeforeTools,
        contentAfterTools,
    };
}

type ChatState = {
    messages: ChatMessage[];
    loading: boolean;
    error: string | null;
    models: ModelInfo[];
    selectedModelId: string;
    chatMode: ChatMode;
    pendingActions: StructuredAction[] | null;
    pendingApprovalRequest: HookApprovalRequest | null;
    waitingForApproval: boolean;
    toolActivity: ToolActivityState | null;
    cognitiveInterrupt: CognitiveInterruptState | null;
    chatActivities: ChatActivity[];
    activeTodos: TodoItem[];
    messageQueue: QueuedRequest[];
};

type ChatAction =
    | { type: 'messages/replace'; messages: ChatMessage[] }
    | { type: 'messages/update'; updater: (messages: ChatMessage[]) => ChatMessage[] }
    | { type: 'loading/set'; loading: boolean }
    | { type: 'error/set'; error: string | null }
    | { type: 'models/set'; models: ModelInfo[] }
    | { type: 'model/set'; modelId: string }
    | { type: 'mode/set'; mode: ChatMode }
    | { type: 'pending-actions/set'; actions: StructuredAction[] | null }
    | { type: 'pending-approval-request/set'; request: HookApprovalRequest | null }
    | { type: 'waiting-for-approval/set'; waiting: boolean }
    | { type: 'tool-activity/set'; activity: ToolActivityState | null }
    | { type: 'cognitive-interrupt/set'; interrupt: CognitiveInterruptState | null }
    | { type: 'chat-activity/upsert'; activity: ChatActivity }
    | { type: 'chat-activity/clear' }
    | { type: 'todos/set'; todos: TodoItem[] }
    | { type: 'queue/enqueue'; request: QueuedRequest }
    | { type: 'queue/shift' }
    | { type: 'queue/delete'; index: number }
    | { type: 'queue/clear' };

const initialState: ChatState = {
    messages: [],
    loading: false,
    error: null,
    models: [],
    selectedModelId: 'anthropic/claude-sonnet-4-5-20250929',
    chatMode: 'code',
    pendingActions: null,
    pendingApprovalRequest: null,
    waitingForApproval: false,
    toolActivity: null,
    cognitiveInterrupt: null,
    chatActivities: [],
    activeTodos: [],
    messageQueue: [],
};

function chatReducer(state: ChatState, action: ChatAction): ChatState {
    switch (action.type) {
        case 'messages/replace':
            return { ...state, messages: action.messages };
        case 'messages/update':
            return { ...state, messages: action.updater(state.messages) };
        case 'loading/set':
            return { ...state, loading: action.loading };
        case 'error/set':
            return { ...state, error: action.error };
        case 'models/set':
            return { ...state, models: action.models };
        case 'model/set':
            return { ...state, selectedModelId: action.modelId };
        case 'mode/set':
            return { ...state, chatMode: action.mode };
        case 'pending-actions/set':
            return { ...state, pendingActions: action.actions };
        case 'pending-approval-request/set':
            return { ...state, pendingApprovalRequest: action.request };
        case 'waiting-for-approval/set':
            return { ...state, waitingForApproval: action.waiting };
        case 'tool-activity/set':
            if (areToolActivitiesEqual(state.toolActivity, action.activity)) {
                return state;
            }
            return { ...state, toolActivity: action.activity };
        case 'cognitive-interrupt/set':
            return state.cognitiveInterrupt === action.interrupt
                ? state
                : { ...state, cognitiveInterrupt: action.interrupt };
        case 'chat-activity/upsert': {
            const existingIndex = state.chatActivities.findIndex((activity) => activity.id === action.activity.id);
            if (existingIndex >= 0) {
                const nextActivities = [...state.chatActivities];
                nextActivities[existingIndex] = action.activity;
                return { ...state, chatActivities: nextActivities };
            }
            return {
                ...state,
                chatActivities: [...state.chatActivities, action.activity].slice(-MAX_CHAT_ACTIVITY_HISTORY),
            };
        }
        case 'chat-activity/clear':
            return state.chatActivities.length === 0 ? state : { ...state, chatActivities: [] };
        case 'todos/set':
            return { ...state, activeTodos: action.todos };
        case 'queue/enqueue':
            return { ...state, messageQueue: [...state.messageQueue, action.request] };
        case 'queue/shift':
            return state.messageQueue.length > 0
                ? { ...state, messageQueue: state.messageQueue.slice(1) }
                : state;
        case 'queue/delete':
            return { ...state, messageQueue: state.messageQueue.filter((_, index) => index !== action.index) };
        case 'queue/clear':
            return { ...state, messageQueue: [] };
        default:
            return state;
    }
}

function buildSystemAssistantMessage(id: string, content: string): ChatMessage {
    return {
        id,
        role: 'Assistant',
        content,
        blocks: [{ type: 'text', content, id: `${id}-text` }],
    };
}

function createPlanSummaryMessage(todos: TodoItem[]): ChatMessage {
    const id = `plan-summary-${Date.now()}`;
    return {
        id,
        role: 'Assistant',
        content: '',
        blocks: [{ type: 'plan_summary', id }],
        planSummary: {
            todos: [...todos],
            completedAt: Date.now(),
        },
    };
}

function imageSignature(image: ChatImage | undefined, index: number): string {
    if (!image) {
        return `missing:${index}`;
    }

    const dataPreview = typeof image.data === 'string' ? image.data.slice(0, 48) : '';
    return [
        image.mime_type || '',
        image.name || '',
        image.size ?? '',
        dataPreview,
        index,
    ].join('|');
}

function messageImageSignature(message: ChatMessage): string | null {
    if (message.role !== 'User' || !message.images || message.images.length === 0) {
        return null;
    }

    return [
        message.role,
        message.content,
        ...message.images.map((image, index) => imageSignature(image, index)),
    ].join('::');
}

function reconcileMessageImagePreviews(previousMessages: ChatMessage[], nextMessages: ChatMessage[]): ChatMessage[] {
    if (previousMessages.length === 0 || nextMessages.length === 0) {
        return nextMessages;
    }

    const previousById = new Map<string, ChatMessage>();
    const previousByImageSignature = new Map<string, ChatMessage>();

    for (const message of previousMessages) {
        if (message.id) {
            previousById.set(message.id, message);
        }
        const signature = messageImageSignature(message);
        if (signature) {
            previousByImageSignature.set(signature, message);
        }
    }

    let changed = false;
    const reconciled = nextMessages.map((message) => {
        if (message.role !== 'User' || !message.images || message.images.length === 0) {
            return message;
        }

        const previous = (message.id ? previousById.get(message.id) : undefined)
            || previousByImageSignature.get(messageImageSignature(message) || '');
        if (!previous?.images || previous.images.length !== message.images.length) {
            return message;
        }

        let imageChanged = false;
        const nextImages = message.images.map((image, index) => {
            const previousImage = previous.images?.[index] as ImageAttachment | undefined;
            if (!previousImage) {
                return image;
            }

            const nextImage = image as ImageAttachment;
            const mergedImage: ImageAttachment = {
                ...nextImage,
                dataUrl: nextImage.dataUrl || previousImage.dataUrl,
                thumbnailUrl: nextImage.thumbnailUrl || previousImage.thumbnailUrl,
            };

            if (mergedImage.dataUrl !== nextImage.dataUrl || mergedImage.thumbnailUrl !== nextImage.thumbnailUrl) {
                imageChanged = true;
            }

            return mergedImage;
        });

        if (!imageChanged) {
            return message;
        }

        changed = true;
        return {
            ...message,
            images: nextImages,
        };
    });

    return changed ? reconciled : nextMessages;
}

export function useChatV2(options: UseChatV2Options = {}) {
    const { getEditorSnapshot } = useEditorActions();
    const [state, dispatch] = useReducer(chatReducer, initialState);

    const getEditorSnapshotRef = useRef(getEditorSnapshot);
    const getActiveBufferContentRef = useRef(options.getActiveBufferContent);
    const firstDispatchRef = useRef(true);
    const messagesRef = useRef<ChatMessage[]>([]);
    const messageByIdRef = useRef<Map<string, ChatMessage>>(new Map());
    const messageIndexByIdRef = useRef<Map<string, number>>(new Map());
    const toolCallOwnerMessageIdRef = useRef<Map<string, string>>(new Map());
    const pendingActionsRef = useRef<StructuredAction[] | null>(null);
    const pendingApprovalRequestRef = useRef<HookApprovalRequest | null>(null);
    const autoApproveRunCommandsRef = useRef(options.autoApproveRunCommands === true);
    const markToolCallsExecutingLocallyRef = useRef<(toolCallIds: string[]) => void>(() => undefined);
    const selectedModelIdRef = useRef(state.selectedModelId);
    const chatModeRef = useRef(state.chatMode);
    const activeTodosRef = useRef<TodoItem[]>(state.activeTodos);
    const toolActivityRef = useRef<ToolActivityState | null>(state.toolActivity);
    const cognitiveInterruptRef = useRef<CognitiveInterruptState | null>(state.cognitiveInterrupt);
    const hasExplicitModelRef = useRef(false);
    const blocksRef = useRef<Map<string, MessageBlock[]>>(new Map());
    const messageBufferRef = useRef<MessageBuffer | null>(null);
    const accumulatedContentRef = useRef<{ id: string; content: string }>({ id: '', content: '' });
    const accumulatedReasoningRef = useRef<{ id: string; content: string }>({ id: '', content: '' });
    const dispatchInFlightRef = useRef(false);
    const loadingRef = useRef(state.loading);
    const messageQueueRef = useRef<QueuedRequest[]>(state.messageQueue);
    const blockedQueuedRequestRef = useRef<QueuedRequest | null>(null);
    const pendingUpdatesRef = useRef<Map<string, {
        content: string;
        reasoning: string;
        blocks: MessageBlock[];
        streaming?: StreamingState;
        contentBeforeTools?: string;
        contentAfterTools?: string;
    }>>(new Map());
    const flushFrameRef = useRef<number | null>(null);
    const streamingStatesRef = useRef<Map<string, StreamingState>>(new Map());
    const toolChunkCountsRef = useRef<Map<string, { chunkCount: number; startedAt: number; lastChunkAt: number }>>(new Map());
    const messageCompletionCleanupTimersRef = useRef<Map<string, number>>(new Map());
    const pendingRunCommandCompletionStatusRef = useRef<Map<string, 'complete' | 'error' | 'skipped'>>(new Map());
    const ignoredChatEventIdsRef = useRef<Set<string>>(new Set());

    const toChatMentions = useCallback((mentions?: ComposerMention[]): ChatMention[] | undefined => {
        if (!mentions || mentions.length === 0) {
            return undefined;
        }
        return mentions.map((mention) => ({
            kind: mention.kind,
            path: mention.path,
            is_dir: mention.is_dir,
        }));
    }, []);
    const pendingTimeoutsRef = useRef<number[]>([]);
    const lastToolActivityDispatchAtRef = useRef(0);

    const setMessages = useCallback((updater: ChatMessage[] | ((messages: ChatMessage[]) => ChatMessage[])) => {
        if (typeof updater === 'function') {
            dispatch({ type: 'messages/update', updater });
            return;
        }
        dispatch({ type: 'messages/replace', messages: updater });
    }, []);

    const replaceMessagesPreservingImagePreviews = useCallback((incomingMessages: ChatMessage[]) => {
        dispatch({
            type: 'messages/replace',
            messages: reconcileMessageImagePreviews(messagesRef.current, incomingMessages),
        });
    }, []);

    const clearPendingTimers = useCallback(() => {
        if (pendingTimeoutsRef.current.length === 0) {
            if (messageCompletionCleanupTimersRef.current.size === 0) {
                return;
            }
        }
        pendingTimeoutsRef.current.forEach((timerId) => window.clearTimeout(timerId));
        pendingTimeoutsRef.current = [];
        messageCompletionCleanupTimersRef.current.forEach((timerId) => window.clearTimeout(timerId));
        messageCompletionCleanupTimersRef.current.clear();
    }, []);

    const cancelMessageCompletionCleanup = useCallback((id: string) => {
        const timerId = messageCompletionCleanupTimersRef.current.get(id);
        if (timerId === undefined) {
            return;
        }
        window.clearTimeout(timerId);
        messageCompletionCleanupTimersRef.current.delete(id);
    }, []);

    const scheduleMessageCompletionCleanup = useCallback((id: string) => {
        cancelMessageCompletionCleanup(id);
        const timerId = window.setTimeout(() => {
            messageCompletionCleanupTimersRef.current.delete(id);
            messageBufferRef.current?.clear(id);
            streamingStatesRef.current.delete(id);
            if (accumulatedContentRef.current.id === id) {
                accumulatedContentRef.current = { id: '', content: '' };
            }
            if (accumulatedReasoningRef.current.id === id) {
                accumulatedReasoningRef.current = { id: '', content: '' };
            }
            toolActivityRef.current = null;
            lastToolActivityDispatchAtRef.current = Date.now();
            dispatch({ type: 'tool-activity/set', activity: null });
            setMessages((messages) => {
                let changed = false;
                const nextMessages = messages.map((message) => {
                    if (message.id !== id || !message.streaming) {
                        return message;
                    }

                    changed = true;
                    return {
                        ...message,
                        streaming: undefined,
                    };
                });
                return changed ? nextMessages : messages;
            });
        }, MESSAGE_COMPLETION_GRACE_MS);
        messageCompletionCleanupTimersRef.current.set(id, timerId);
    }, [cancelMessageCompletionCleanup, setMessages]);

    const setToolActivity = useCallback((activity: ToolActivityState | null) => {
        const previous = toolActivityRef.current;
        toolActivityRef.current = activity;

        if (areToolActivitiesEqual(previous, activity)) {
            return;
        }

        if (!activity) {
            lastToolActivityDispatchAtRef.current = Date.now();
            dispatch({ type: 'tool-activity/set', activity: null });
            return;
        }

        const now = Date.now();
        const previousKey = previous ? `${previous.toolCallId || `${previous.toolName}:${previous.filePath}`}:${previous.action}` : '';
        const nextKey = `${activity.toolCallId || `${activity.toolName}:${activity.filePath}`}:${activity.action}`;
        const shouldDispatch = !previous
            || previousKey !== nextKey
            || activity.action !== 'streaming'
            || activity.chunkCount <= 1
            || (now - lastToolActivityDispatchAtRef.current) >= TOOL_ACTIVITY_DISPATCH_INTERVAL_MS;

        if (!shouldDispatch) {
            return;
        }

        lastToolActivityDispatchAtRef.current = now;
        dispatch({ type: 'tool-activity/set', activity });
    }, []);

    useEffect(() => {
        getEditorSnapshotRef.current = getEditorSnapshot;
    }, [getEditorSnapshot]);

    useEffect(() => {
        getActiveBufferContentRef.current = options.getActiveBufferContent;
    }, [options.getActiveBufferContent]);

    useEffect(() => {
        messagesRef.current = state.messages;
        const nextMessageById = new Map<string, ChatMessage>();
        const nextMessageIndexById = new Map<string, number>();
        const nextToolCallOwnerMessageId = new Map<string, string>();

        state.messages.forEach((message, index) => {
            if (message.id) {
                nextMessageById.set(message.id, message);
                nextMessageIndexById.set(message.id, index);
                (message.tool_calls || []).forEach((toolCall) => {
                    nextToolCallOwnerMessageId.set(toolCall.id, message.id!);
                });
            }
        });

        messageByIdRef.current = nextMessageById;
        messageIndexByIdRef.current = nextMessageIndexById;
        toolCallOwnerMessageIdRef.current = nextToolCallOwnerMessageId;
    }, [state.messages]);

    useEffect(() => {
        pendingActionsRef.current = state.pendingActions;
    }, [state.pendingActions]);

    useEffect(() => {
        pendingApprovalRequestRef.current = state.pendingApprovalRequest;
    }, [state.pendingApprovalRequest]);

    useEffect(() => {
        autoApproveRunCommandsRef.current = options.autoApproveRunCommands === true;
    }, [options.autoApproveRunCommands]);

    useEffect(() => {
        selectedModelIdRef.current = state.selectedModelId;
    }, [state.selectedModelId]);

    useEffect(() => {
        loadingRef.current = state.loading;
    }, [state.loading]);

    useEffect(() => {
        messageQueueRef.current = state.messageQueue;
    }, [state.messageQueue]);

    useEffect(() => {
        chatModeRef.current = state.chatMode;
    }, [state.chatMode]);

    useEffect(() => {
        activeTodosRef.current = state.activeTodos;
    }, [state.activeTodos]);

    useEffect(() => {
        toolActivityRef.current = state.toolActivity;
    }, [state.toolActivity]);

    useEffect(() => {
        cognitiveInterruptRef.current = state.cognitiveInterrupt;
    }, [state.cognitiveInterrupt]);

    const buildEditorContext = useCallback((snapshot: {
        activeFile: string | null;
        openFiles: string[];
        cursorLine: number | null;
        cursorColumn: number | null;
        selectionStartLine: number | null;
        selectionEndLine: number | null;
    }) => {
        const safeActiveFile = snapshot.activeFile || null;
        const openFromState = snapshot.openFiles.length > 0
            ? snapshot.openFiles
            : (safeActiveFile ? [safeActiveFile] : []);
        const normalizedOpenFiles = Array.from(new Set(openFromState.filter(Boolean)));

        // Dirty active-file buffer (null when clean/unavailable). The backend
        // hashes it for the workspace payload; it is never forwarded to zcoderd.
        const activeBufferContent = safeActiveFile
            ? (getActiveBufferContentRef.current?.() ?? null)
            : null;

        return {
            active_file: safeActiveFile,
            open_files: normalizedOpenFiles,
            cursor_line: snapshot.cursorLine ?? null,
            cursor_column: snapshot.cursorColumn ?? null,
            selection_start: snapshot.selectionStartLine ?? null,
            selection_end: snapshot.selectionEndLine ?? null,
            diagnostics: getActiveDiagnostics(safeActiveFile, normalizedOpenFiles),
            active_buffer_content: activeBufferContent,
        };
    }, []);

    const mergeEditorSnapshots = useCallback((freshSnapshot: {
        active_file: string | null;
        open_files: string[];
        cursor_line: number | null;
        cursor_column: number | null;
        selection_start: number | null;
        selection_end: number | null;
    }, cachedSnapshot: ReturnType<typeof getEditorSnapshotRef.current>) => {
        const backendOwnsEditorState = isBackendAuthoritative();
        const activeFile = backendOwnsEditorState
            ? freshSnapshot.active_file ?? cachedSnapshot.activeFile ?? null
            : cachedSnapshot.activeFile ?? null;
        const freshOpenFiles = freshSnapshot.open_files || [];
        const cachedOpenFiles = cachedSnapshot.openFiles || [];
        const openFilesSource = backendOwnsEditorState
            ? (freshOpenFiles.length > 0 ? freshOpenFiles : cachedOpenFiles)
            : cachedOpenFiles;
        const openFiles = Array.from(new Set(openFilesSource.filter(Boolean)));

        return {
            activeFile,
            openFiles,
            cursorLine: freshSnapshot.cursor_line ?? cachedSnapshot.cursorLine,
            cursorColumn: freshSnapshot.cursor_column ?? cachedSnapshot.cursorColumn,
            selectionStartLine: freshSnapshot.selection_start ?? cachedSnapshot.selectionStartLine,
            selectionEndLine: freshSnapshot.selection_end ?? cachedSnapshot.selectionEndLine,
        };
    }, []);

    const requestFreshEditorContext = useCallback(async () => {
        if (!firstDispatchRef.current) {
            return buildEditorContext(getEditorSnapshotRef.current());
        }

        try {
            const cachedSnapshot = getEditorSnapshotRef.current();
            const snapshotPromise = waitForBladeEvent((envelope) => {
                if (envelope.event.type !== 'Editor') {
                    return false;
                }

                const payload = envelope.event.payload as { type: string; payload: any };
                return payload.type === 'StateSnapshot';
            }, 300);

            await EditorFacade.getState();
            const snapshotEnvelope = await snapshotPromise;
            const snapshot = (snapshotEnvelope.event.payload as { type: 'StateSnapshot'; payload: {
                active_file: string | null;
                open_files: string[];
                cursor_line: number | null;
                cursor_column: number | null;
                selection_start: number | null;
                selection_end: number | null;
            } }).payload;
            return buildEditorContext(mergeEditorSnapshots(snapshot, cachedSnapshot));
        } catch {
            return buildEditorContext(getEditorSnapshotRef.current());
        }
    }, [buildEditorContext, mergeEditorSnapshots]);

    const syncSelectedModel = useCallback(async (modelId: string, explicit: boolean) => {
        const isSameSelection = selectedModelIdRef.current === modelId;
        const isSameExplicitState = hasExplicitModelRef.current === explicit;
        if (isSameSelection && isSameExplicitState) {
            return;
        }

        hasExplicitModelRef.current = explicit;
        selectedModelIdRef.current = modelId;
        dispatch({ type: 'model/set', modelId });
        try {
            await BladeDispatcher.chat({
                type: 'SetSelectedModel',
                payload: { model: modelId },
            });
        } catch (error) {
            console.error('[useChatV2] Failed to sync model to backend:', error);
        }
    }, []);

    const setLocalSelectedModel = useCallback((modelId: string, explicit: boolean) => {
        if (selectedModelIdRef.current === modelId && hasExplicitModelRef.current === explicit) {
            return;
        }

        hasExplicitModelRef.current = explicit;
        selectedModelIdRef.current = modelId;
        dispatch({ type: 'model/set', modelId });
    }, []);

    const refreshModels = useCallback(async () => {
        const models = await invoke<ModelInfo[]>('list_models');
        dispatch({ type: 'models/set', models });
        writeCachedModels(models);

        if (models.length === 0) {
            return models;
        }

        const hasSelectedModel = models.some((model) => (
            model.id === selectedModelIdRef.current || model.api_id === selectedModelIdRef.current
        ));

        if (!hasSelectedModel) {
            const defaultModel = pickDefaultModel(models);
            if (defaultModel) {
                setLocalSelectedModel(defaultModel.id, false);
            }
        }

        return models;
    }, [setLocalSelectedModel]);

    const resetStreamingState = useCallback(() => {
        messageBufferRef.current?.clearAll();
        accumulatedContentRef.current = { id: '', content: '' };
        accumulatedReasoningRef.current = { id: '', content: '' };
        blocksRef.current.clear();
        pendingUpdatesRef.current.clear();
        streamingStatesRef.current.clear();
        toolChunkCountsRef.current.clear();
        if (flushFrameRef.current !== null) {
            window.cancelAnimationFrame(flushFrameRef.current);
            flushFrameRef.current = null;
        }
        clearPendingTimers();
        pendingRunCommandCompletionStatusRef.current.clear();
        toolActivityRef.current = null;
        lastToolActivityDispatchAtRef.current = Date.now();
        dispatchInFlightRef.current = false;
        dispatch({ type: 'loading/set', loading: false });
        dispatch({ type: 'pending-actions/set', actions: null });
    }, [clearPendingTimers]);

    const flushPendingUpdates = useCallback(() => {
        flushFrameRef.current = null;
        const pending = pendingUpdatesRef.current;
        if (pending.size === 0) {
            return;
        }
        setMessages((previousMessages) => {
            let nextMessages = previousMessages;
            let changed = false;
            const indexById = new Map<string, number>();

            for (let index = 0; index < previousMessages.length; index += 1) {
                const messageId = previousMessages[index].id;
                if (messageId) {
                    indexById.set(messageId, index);
                }
            }

            pending.forEach((update, id) => {
                const index = indexById.get(id) ?? -1;
                if (index !== -1) {
                    const existingMessage = nextMessages[index];
                    const streamingChanged =
                        (existingMessage.streaming?.seq ?? null) !== (update.streaming?.seq ?? null)
                        || (existingMessage.streaming?.endTime ?? null) !== (update.streaming?.endTime ?? null)
                        || (existingMessage.streaming?.activeKind ?? null) !== (update.streaming?.activeKind ?? null)
                        || (existingMessage.streaming?.activeBlockId ?? null) !== (update.streaming?.activeBlockId ?? null);

                    if (
                        existingMessage.content !== update.content
                        || existingMessage.reasoning !== update.reasoning
                        || existingMessage.content_before_tools !== update.contentBeforeTools
                        || existingMessage.content_after_tools !== update.contentAfterTools
                        || streamingChanged
                    ) {
                        if (!changed) {
                            nextMessages = [...previousMessages];
                            changed = true;
                        }

                        const updateBlockIds = new Set(update.blocks.map((block) => block.id));
                        const missingNonTextBlocks = (existingMessage.blocks || []).filter(
                            (block) => block.type !== 'text' && block.type !== 'reasoning' && !updateBlockIds.has(block.id),
                        );
                        const mergedBlocks = [...update.blocks];

                        if (missingNonTextBlocks.length > 0) {
                            let insertIndex = mergedBlocks.length;
                            for (let blockIndex = mergedBlocks.length - 1; blockIndex >= 0; blockIndex -= 1) {
                                if (mergedBlocks[blockIndex].type !== 'text' && mergedBlocks[blockIndex].type !== 'reasoning') {
                                    insertIndex = blockIndex + 1;
                                    break;
                                }
                            }
                            mergedBlocks.splice(insertIndex, 0, ...missingNonTextBlocks);
                        }

                        nextMessages[index] = {
                            ...existingMessage,
                            content: update.content,
                            reasoning: update.reasoning,
                            content_before_tools: update.contentBeforeTools,
                            content_after_tools: update.contentAfterTools,
                            blocks: mergedBlocks,
                            streaming: update.streaming,
                        };
                    }
                    return;
                }

                if (!changed) {
                    nextMessages = [...previousMessages];
                    changed = true;
                }

                nextMessages.push({
                    id,
                    role: 'Assistant',
                    content: update.content,
                    reasoning: update.reasoning,
                    content_before_tools: update.contentBeforeTools,
                    content_after_tools: update.contentAfterTools,
                    blocks: update.blocks,
                    streaming: update.streaming,
                });
            });

            pending.clear();
            return changed ? nextMessages : previousMessages;
        });
    }, [setMessages]);

    const flushPendingUpdatesImmediately = useCallback(() => {
        if (flushFrameRef.current !== null) {
            window.cancelAnimationFrame(flushFrameRef.current);
            flushFrameRef.current = null;
        }
        flushPendingUpdates();
    }, [flushPendingUpdates]);

    const finalizeActiveStreamingMessages = useCallback(() => {
        flushPendingUpdatesImmediately();

        const now = Date.now();
        const activeIds = new Set<string>();
        streamingStatesRef.current.forEach((_streaming, id) => activeIds.add(id));
        messagesRef.current.forEach((message) => {
            if (message.id && message.streaming && !message.streaming.endTime) {
                activeIds.add(message.id);
            }
        });

        if (activeIds.size === 0) {
            return;
        }

        activeIds.forEach((id) => {
            const existingStreaming = streamingStatesRef.current.get(id)
                ?? messageByIdRef.current.get(id)?.streaming;
            if (existingStreaming) {
                streamingStatesRef.current.set(id, {
                    ...existingStreaming,
                    endTime: existingStreaming.endTime ?? now,
                });
            }
            scheduleMessageCompletionCleanup(id);
        });

        setMessages((messages) => {
            let changed = false;
            const nextMessages = messages.map((message) => {
                if (!message.id || !activeIds.has(message.id) || !message.streaming || message.streaming.endTime) {
                    return message;
                }

                changed = true;
                return {
                    ...message,
                    streaming: {
                        ...message.streaming,
                        endTime: now,
                    },
                };
            });

            return changed ? nextMessages : messages;
        });
    }, [flushPendingUpdatesImmediately, scheduleMessageCompletionCleanup, setMessages]);

    const scheduleFlush = useCallback(() => {
        if (flushFrameRef.current !== null) {
            return;
        }

        flushFrameRef.current = window.requestAnimationFrame(() => {
            flushFrameRef.current = null;
            flushPendingUpdates();
        });
    }, [flushPendingUpdates]);

    const queueMessageUpdate = useCallback((
        id: string,
        content: string,
        reasoning: string,
        blocks: MessageBlock[],
        streaming?: StreamingState,
        contentBeforeTools?: string,
        contentAfterTools?: string,
    ) => {
        pendingUpdatesRef.current.set(id, {
            content,
            reasoning,
            blocks,
            streaming,
            contentBeforeTools,
            contentAfterTools,
        });
        scheduleFlush();
    }, [scheduleFlush]);

    const setSelectedModelId = useCallback(async (modelId: string) => {
        await syncSelectedModel(modelId, true);
    }, [syncSelectedModel]);

    const updateMessages = useCallback((updater: (messages: ChatMessage[]) => ChatMessage[]) => {
        setMessages(updater);
    }, [setMessages]);

    const applyDeferredRunCommandCompletion = useCallback((toolCallId: string) => {
        const pendingStatus = pendingRunCommandCompletionStatusRef.current.get(toolCallId);
        if (!pendingStatus) {
            return;
        }

        pendingRunCommandCompletionStatusRef.current.delete(toolCallId);
        updateMessages((messages) => messages.map((message) => {
            if (!message.tool_calls?.some((toolCall) => toolCall.id === toolCallId)) {
                return message;
            }
            return {
                ...message,
                tool_calls: (message.tool_calls || []).map((toolCall) => toolCall.id === toolCallId
                    ? {
                        ...toolCall,
                        status: pendingStatus,
                        ...(pendingStatus === 'skipped' && !toolCall.result ? { result: i18n.t('toolCall.skippedByUser', { defaultValue: 'Skipped by user' }) } : {}),
                    }
                    : toolCall),
            };
        }));
    }, [updateMessages]);

    const updateToolCallsStatusLocally = useCallback((
        toolCallIds: string[],
        nextStatus: 'pending' | 'executing' | 'complete' | 'error' | 'skipped',
        fallbackResultText?: string,
    ) => {
        if (toolCallIds.length === 0) {
            return;
        }
        const idSet = new Set(toolCallIds);
        updateMessages((messages) => messages.map((message) => {
            if (!message.tool_calls?.some((toolCall) => idSet.has(toolCall.id))) {
                return message;
            }
            return {
                ...message,
                tool_calls: (message.tool_calls || []).map((toolCall) => idSet.has(toolCall.id)
                    ? {
                        ...toolCall,
                        status: nextStatus,
                        ...(!toolCall.result && fallbackResultText ? { result: fallbackResultText } : {}),
                    }
                    : toolCall),
            };
        }));
    }, [updateMessages]);

    const markToolCallsExecutingLocally = useCallback((toolCallIds: string[]) => {
        updateToolCallsStatusLocally(toolCallIds, 'executing');
    }, [updateToolCallsStatusLocally]);

    useEffect(() => {
        markToolCallsExecutingLocallyRef.current = markToolCallsExecutingLocally;
    }, [markToolCallsExecutingLocally]);

    const markToolCallsSkippedLocally = useCallback((toolCallIds: string[], skippedResultText = i18n.t('toolCall.skippedByUser', { defaultValue: 'Skipped by user' })) => {
        updateToolCallsStatusLocally(toolCallIds, 'skipped', skippedResultText);
    }, [updateToolCallsStatusLocally]);

    useEffect(() => {
        async function init() {
            try {
                if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
                    return;
                }

                ensureProblemCaptureStarted();

                const cachedModels = readCachedModels();
                if (cachedModels.length > 0) {
                    dispatch({ type: 'models/set', models: cachedModels });
                }

                const [history, isStreaming] = await Promise.all([
                    invoke<ChatMessage[]>('get_conversation'),
                    invoke<boolean>('get_chat_status'),
                ]);

                const migratedHistory = ensureMessagesHaveBlocks(history);
                replaceMessagesPreservingImagePreviews(migratedHistory);

                if (isStreaming) {
                    dispatch({ type: 'loading/set', loading: true });
                }
            } catch (error) {
                console.error('[useChatV2] Failed to initialize chat:', error);
            }

            void refreshModels().catch((error) => {
                console.error('[useChatV2] Failed to refresh models in background:', error);
            });
        }

        void init();
    }, [refreshModels]);

    const [hiddenLocalModels, setHiddenLocalModels] = useState<string[]>([]);

    const loadHiddenLocalModels = useCallback(async () => {
        try {
            const config = await invoke<{ hidden_local_models?: string[] }>('get_local_ai_settings');
            setHiddenLocalModels(config.hidden_local_models ?? []);
        } catch (error) {
            console.error('[useChatV2] Failed to load hidden local models:', error);
        }
    }, []);

    useEffect(() => {
        void loadHiddenLocalModels();
    }, [loadHiddenLocalModels]);

    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
            return;
        }

        const unlistenPromise = listen('local-ai-settings-changed', () => {
            void loadHiddenLocalModels();
            void refreshModels().catch((error) => {
                console.error('[useChatV2] Failed to refresh models after local AI settings changed:', error);
            });
        });

        return () => {
            unlistenPromise.then((unlisten) => unlisten());
        };
    }, [refreshModels, loadHiddenLocalModels]);

    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
            return;
        }

        if (!messageBufferRef.current) {
            messageBufferRef.current = new MessageBuffer(
                (id, seq, chunk, _isFinal, type) => {
                    cancelMessageCompletionCleanup(id);
                    const now = Date.now();
                    const previousStreaming = streamingStatesRef.current.get(id);
                    let streaming: StreamingState = {
                        seq: Math.max(seq, previousStreaming?.seq ?? 0),
                        startTime: previousStreaming?.startTime ?? now,
                        lastSeqAt: now,
                        activeKind: type,
                    };
                    streamingStatesRef.current.set(id, streaming);

                    // Delta-mode append: the backend sends true deltas sequenced
                    // via EventBuffer, so we simply append each chunk.
                    if (type === 'reasoning') {
                        if (accumulatedReasoningRef.current.id !== id) {
                            accumulatedReasoningRef.current = { id, content: '' };
                        }
                        accumulatedReasoningRef.current.content += chunk;
                        if (STREAM_DEBUG_ENABLED) {
                            streamDebugLog('[stream-debug][ui-merge][reasoning]', {
                                id,
                                seq,
                                incomingLength: chunk.length,
                                previousLength: accumulatedReasoningRef.current.content.length - chunk.length,
                                appendLength: chunk.length,
                                changed: chunk.length > 0,
                                incomingPreview: streamDebugPreview(chunk),
                                nextPreview: streamDebugPreview(accumulatedReasoningRef.current.content),
                            });
                        }
                    } else {
                        if (accumulatedContentRef.current.id !== id) {
                            accumulatedContentRef.current = { id, content: '' };
                        }
                        accumulatedContentRef.current.content += chunk;
                        if (STREAM_DEBUG_ENABLED) {
                            streamDebugLog('[stream-debug][ui-merge][text]', {
                                id,
                                seq,
                                incomingLength: chunk.length,
                                previousLength: accumulatedContentRef.current.content.length - chunk.length,
                                appendLength: chunk.length,
                                changed: chunk.length > 0,
                                incomingPreview: streamDebugPreview(chunk),
                                nextPreview: streamDebugPreview(accumulatedContentRef.current.content),
                            });
                        }
                    }

                    if (chunk.length === 0) {
                        return;
                    }

                    // --- Block assembly ---
                    const existingMessage = messageByIdRef.current.get(id);
                    let blocks = blocksRef.current.get(id) || [];

                    if (blocks.length === 0 && existingMessage?.blocks && existingMessage.blocks.length > 0) {
                        blocks = existingMessage.blocks.filter((block) => block.type !== 'text' && block.type !== 'reasoning');
                    }

                    if (type === 'reasoning') {
                        // Drop trailing whitespace-only text block before reasoning
                        const lastBlock = blocks[blocks.length - 1];
                        if (lastBlock?.type === 'text' && isWhitespaceOnly(lastBlock.content)) {
                            blocks = blocks.slice(0, -1);
                        }
                        // Find the last reasoning block to append to (may not be the very last block)
                        let lastReasoningIndex = -1;
                        for (let index = blocks.length - 1; index >= 0; index -= 1) {
                            if (blocks[index].type === 'reasoning') {
                                lastReasoningIndex = index;
                                break;
                            }
                        }
                        // If the last reasoning block is separated only by whitespace-only text blocks, continue it
                        const lastBlock2 = blocks[blocks.length - 1];
                        if (lastBlock2?.type === 'reasoning') {
                            blocks[blocks.length - 1] = { ...lastBlock2, content: lastBlock2.content + chunk };
                        } else if (lastReasoningIndex >= 0 && blocks.slice(lastReasoningIndex + 1).every((b) => b.type === 'text' && isWhitespaceOnly(b.content))) {
                            // All blocks after last reasoning are whitespace text - continue the reasoning block
                            const targetBlock = blocks[lastReasoningIndex];
                            if (targetBlock.type === 'reasoning') {
                                // Remove whitespace-only text blocks between reasoning blocks
                                blocks = [...blocks.slice(0, lastReasoningIndex), { ...targetBlock, content: targetBlock.content + chunk }, ...blocks.slice(lastReasoningIndex + 1).filter((b) => !(b.type === 'text' && isWhitespaceOnly(b.content)))];
                            }
                        } else if (lastReasoningIndex >= 0) {
                            // There is an existing reasoning block but it's separated by real (non-whitespace)
                            // text or activity blocks. Merge the new reasoning chunk into the existing
                            // reasoning block instead of creating a second card. All intervening blocks
                            // (text, tool calls, etc.) stay in place, so visual ordering is preserved.
                            // This prevents the annoying split where a tiny trailing reasoning fragment
                            // (e.g. a single word or ".") appears as a separate collapsed card after
                            // the main response text.
                            const targetBlock = blocks[lastReasoningIndex];
                            if (targetBlock.type === 'reasoning') {
                                blocks = [...blocks.slice(0, lastReasoningIndex), { ...targetBlock, content: targetBlock.content + chunk }, ...blocks.slice(lastReasoningIndex + 1)];
                            }
                        } else {
                            blocks = [...blocks, { type: 'reasoning', content: chunk, id: crypto.randomUUID() }];
                        }
                    } else {
                        const fullContent = accumulatedContentRef.current.content;
                        const normalizedSplitState = normalizeSplitBlocks(existingMessage, blocks, fullContent);
                        const effectiveBlocks = normalizedSplitState.blocks;
                        const hasExistingTextBlock = effectiveBlocks.some((block) => block.type === 'text');
                        const visibleContent = normalizedSplitState.contentBeforeTools !== undefined
                            ? normalizedSplitState.contentAfterTools || ''
                            : fullContent;
                        // Defer creating a text block until we have non-whitespace content
                        if (!hasExistingTextBlock && isWhitespaceOnly(visibleContent)) {
                            blocksRef.current.set(id, effectiveBlocks);
                            queueMessageUpdate(
                                id,
                                accumulatedContentRef.current.id === id ? accumulatedContentRef.current.content : '',
                                accumulatedReasoningRef.current.id === id ? accumulatedReasoningRef.current.content : '',
                                effectiveBlocks,
                                streaming,
                                normalizedSplitState.contentBeforeTools,
                                normalizedSplitState.contentAfterTools,
                            );
                            return;
                        }
                        if (normalizedSplitState.contentBeforeTools !== undefined) {
                            blocks = normalizedSplitState.blocks;
                        } else {
                        // Find the last text block to continue (not necessarily the very last block)
                            let lastTextIndex = -1;
                            for (let index = blocks.length - 1; index >= 0; index -= 1) {
                                if (blocks[index].type === 'text') {
                                    lastTextIndex = index;
                                    break;
                                }
                            }
                            if (lastTextIndex >= 0) {
                                // Update existing text block with full accumulated content
                                const targetBlock = blocks[lastTextIndex];
                                if (targetBlock.type === 'text') {
                                    blocks[lastTextIndex] = { ...targetBlock, content: fullContent };
                                }
                            } else {
                                // Create new text block with full accumulated content
                                blocks = [...blocks, { type: 'text', content: fullContent, id: crypto.randomUUID() }];
                            }
                        }
                    }

                    blocksRef.current.set(id, blocks);
                    const normalizedBlocks = normalizeSplitBlocks(existingMessage, blocks, accumulatedContentRef.current.id === id ? accumulatedContentRef.current.content : '');
                    streaming = {
                        ...streaming,
                        activeBlockId: findLastBlockIdOfType(
                            normalizedBlocks.blocks,
                            type === 'reasoning' ? 'reasoning' : 'text',
                        ),
                    };
                    streamingStatesRef.current.set(id, streaming);
                    queueMessageUpdate(
                        id,
                        accumulatedContentRef.current.id === id ? accumulatedContentRef.current.content : '',
                        accumulatedReasoningRef.current.id === id ? accumulatedReasoningRef.current.content : '',
                        normalizedBlocks.blocks,
                        streaming,
                        normalizedBlocks.contentBeforeTools,
                        normalizedBlocks.contentAfterTools,
                    );
                    if (
                        !existingMessage
                        && normalizedBlocks.blocks.some((block) => block.type === 'text' && !isWhitespaceOnly(block.content))
                    ) {
                        flushPendingUpdatesImmediately();
                    }
                },
                (id) => {
                    const previousStreaming = streamingStatesRef.current.get(id);
                    const streaming = previousStreaming
                        ? { ...previousStreaming, endTime: Date.now() }
                        : undefined;

                    if (streaming) {
                        streamingStatesRef.current.set(id, streaming);
                    }

                    const existingMessage = messageByIdRef.current.get(id);
                    const blocks = blocksRef.current.get(id) || existingMessage?.blocks || [];
                    const fullContent = accumulatedContentRef.current.id === id
                        ? accumulatedContentRef.current.content
                        : (existingMessage?.content || '');
                    const normalizedBlocks = normalizeSplitBlocks(existingMessage, blocks, fullContent);
                    queueMessageUpdate(
                        id,
                        fullContent,
                        accumulatedReasoningRef.current.id === id ? accumulatedReasoningRef.current.content : (existingMessage?.reasoning || ''),
                        normalizedBlocks.blocks,
                        streaming,
                        normalizedBlocks.contentBeforeTools,
                        normalizedBlocks.contentAfterTools,
                    );
                    flushPendingUpdatesImmediately();
                    blocksRef.current.delete(id);
                },
            );
        }

        let unlistenDone: (() => void) | undefined;
        let unlistenError: (() => void) | undefined;
        let unlistenContextLength: (() => void) | undefined;
        let unlistenMessageTooLarge: (() => void) | undefined;
        let unlistenPermission: (() => void) | undefined;
        let unlistenApprovalRequest: (() => void) | undefined;
        let unlistenCommand: (() => void) | undefined;
        let unlistenToolCompleted: (() => void) | undefined;
        let unlistenTodoUpdated: (() => void) | undefined;
        let unlistenBladeEvent: (() => void) | undefined;

        const setupListeners = async () => {
            unlistenDone = await listen<ChatDonePayload>('chat-done', (event) => {
                const finishReason = event.payload?.finish_reason || 'stop';
                dispatchInFlightRef.current = false;
                dispatch({ type: 'loading/set', loading: false });
                dispatch({ type: 'waiting-for-approval/set', waiting: finishReason === 'approval_required' });
                if (finishReason !== 'approval_required') {
                    dispatch({ type: 'pending-actions/set', actions: null });
                    dispatch({ type: 'pending-approval-request/set', request: null });
                }

                if (finishReason === 'approval_required') {
                    return;
                }

                finalizeActiveStreamingMessages();
                toolActivityRef.current = null;
                lastToolActivityDispatchAtRef.current = Date.now();
                dispatch({ type: 'tool-activity/set', activity: null });

                const settleTodosTimer = window.setTimeout(() => {
                    const currentTodos = activeTodosRef.current;
                    if (currentTodos.length === 0 || currentTodos.every((todo) => todo.status === 'completed')) {
                        return;
                    }
                    const completed = currentTodos.map((todo) => ({ ...todo, status: 'completed' as const }));
                    dispatch({ type: 'todos/set', todos: completed });
                    const finalizeTimer = window.setTimeout(() => {
                        dispatch({ type: 'todos/set', todos: [] });
                        updateMessages((messages) => [...messages, createPlanSummaryMessage(completed)]);
                    }, 1500);
                    pendingTimeoutsRef.current.push(finalizeTimer);
                }, 500);
                pendingTimeoutsRef.current.push(settleTodosTimer);
            });

            unlistenError = await listen<ChatErrorPayload>('chat-error', (event) => {
                const errorText = formatChatErrorPayload(event.payload);
                const inFlightToolCallIds = Array.from(new Set(
                    messagesRef.current.flatMap((message) => (message.tool_calls || [])
                        .filter((toolCall) => toolCall.status === undefined || toolCall.status === 'pending' || toolCall.status === 'executing')
                        .map((toolCall) => toolCall.id)),
                ));
                if (inFlightToolCallIds.length > 0) {
                    updateToolCallsStatusLocally(inFlightToolCallIds, 'error', errorText);
                }
                finalizeActiveStreamingMessages();
                toolActivityRef.current = null;
                lastToolActivityDispatchAtRef.current = Date.now();
                dispatchInFlightRef.current = false;
                dispatch({ type: 'loading/set', loading: false });
                dispatch({ type: 'pending-actions/set', actions: null });
                dispatch({ type: 'pending-approval-request/set', request: null });
                dispatch({ type: 'waiting-for-approval/set', waiting: false });
                dispatch({ type: 'tool-activity/set', activity: null });
                dispatch({ type: 'error/set', error: errorText });
            });

            unlistenContextLength = await listen<ContextLengthExceededPayload>('context-length-exceeded', (event) => {
                finalizeActiveStreamingMessages();
                toolActivityRef.current = null;
                lastToolActivityDispatchAtRef.current = Date.now();
                dispatchInFlightRef.current = false;
                dispatch({ type: 'loading/set', loading: false });
                dispatch({ type: 'pending-actions/set', actions: null });
                dispatch({ type: 'tool-activity/set', activity: null });
                dispatch({ type: 'pending-approval-request/set', request: null });
                dispatch({ type: 'waiting-for-approval/set', waiting: false });
                updateMessages((messages) => [
                    ...messages,
                    buildSystemAssistantMessage(
                        `system-context-${Date.now()}`,
                        buildContextLengthSystemMessage(event.payload),
                    ),
                ]);
            });

            unlistenMessageTooLarge = await listen<MessageTooLargePayload>('message-too-large', (event) => {
                finalizeActiveStreamingMessages();
                toolActivityRef.current = null;
                lastToolActivityDispatchAtRef.current = Date.now();
                dispatchInFlightRef.current = false;
                dispatch({ type: 'loading/set', loading: false });
                dispatch({ type: 'pending-actions/set', actions: null });
                dispatch({ type: 'tool-activity/set', activity: null });
                dispatch({ type: 'pending-approval-request/set', request: null });
                dispatch({ type: 'waiting-for-approval/set', waiting: false });
                updateMessages((messages) => [
                    ...messages,
                    buildSystemAssistantMessage(
                        `system-size-${Date.now()}`,
                        buildMessageTooLargeSystemMessage(event.payload),
                    ),
                ]);
            });

            unlistenPermission = await listen<RequestConfirmationPayload>('request-confirmation', (event) => {
                if (
                    autoApproveRunCommandsRef.current
                    && event.payload.actions.length > 0
                    && event.payload.actions.every(isRunCommandAction)
                ) {
                    dispatch({ type: 'pending-actions/set', actions: null });
                    dispatch({ type: 'waiting-for-approval/set', waiting: false });
                    void BladeDispatcher.workflow({
                        type: 'ApproveToolDecision',
                        payload: { decision: 'approve_once' },
                    }).catch((error) => {
                        console.error('[useChatV2] Failed to auto-approve command decision:', error);
                    });
                    return;
                }
                dispatch({ type: 'pending-actions/set', actions: event.payload.actions });
            });

            const normalizeApprovalRequest = (payload: ApprovalRequestPayload): HookApprovalRequest => ({
                sessionId: payload.session_id,
                approvalId: payload.approval_id,
                toolCallId: payload.tool_call_id,
                toolName: payload.tool_name,
                arguments: payload.arguments,
                source: payload.source,
                ruleName: payload.rule_name,
                message: payload.message,
                decision: payload.decision,
            });

            unlistenApprovalRequest = await listen<ApprovalRequestPayload>(EventNames.APPROVAL_REQUEST, (event) => {
                const request = normalizeApprovalRequest(event.payload);
                if (autoApproveRunCommandsRef.current && request.toolName === 'run_command') {
                    markToolCallsExecutingLocallyRef.current([request.toolCallId]);
                    dispatch({ type: 'pending-approval-request/set', request: null });
                    dispatch({ type: 'waiting-for-approval/set', waiting: false });
                    void invoke('send_approval_response', {
                        sessionId: request.sessionId,
                        approvalId: request.approvalId,
                        toolCallId: request.toolCallId,
                        approved: true,
                    }).catch((error) => {
                        console.error('[useChatV2] Failed to auto-approve command request:', error);
                    });
                    return;
                }
                dispatch({ type: 'pending-approval-request/set', request });
                dispatch({ type: 'waiting-for-approval/set', waiting: true });
                updateMessages((messages) => messages.map((message) => {
                    if (!message.tool_calls?.some((toolCall) => toolCall.id === request.toolCallId)) {
                        return message;
                    }
                    return {
                        ...message,
                        tool_calls: (message.tool_calls || []).map((toolCall) => toolCall.id === request.toolCallId
                            ? { ...toolCall, status: 'pending', result: undefined }
                            : toolCall),
                    };
                }));
            });

            unlistenCommand = await listen<{
                command: string;
                cwd?: string;
                output: string;
                exitCode: number;
                duration?: number;
                call_id: string;
            }>('command-executed', (event) => {
                const { command, cwd, output, exitCode, duration, call_id: callId } = event.payload;
                updateMessages((messages) => {
                    const targetMessageId = toolCallOwnerMessageIdRef.current.get(callId);
                    let messageIndex = targetMessageId ? (messageIndexByIdRef.current.get(targetMessageId) ?? -1) : -1;
                    if (messageIndex >= messages.length || (messageIndex >= 0 && messages[messageIndex]?.id !== targetMessageId)) {
                        messageIndex = -1;
                    }
                    if (messageIndex === -1) {
                        messageIndex = messages.findIndex((message) => message.tool_calls?.some((toolCall) => toolCall.id === callId));
                    }
                    if (messageIndex === -1) {
                        return messages;
                    }

                    const nextMessages = [...messages];
                    const message = { ...nextMessages[messageIndex] };
                    const execution: CommandExecution = {
                        id: callId,
                        command,
                        cwd,
                        output,
                        exitCode,
                        duration,
                        timestamp: Date.now(),
                    };

                    const executions = [...(message.commandExecutions || [])];
                    const existingExecutionIndex = executions.findIndex((item) => item.id === callId);
                    if (existingExecutionIndex >= 0) {
                        executions[existingExecutionIndex] = execution;
                    } else {
                        executions.push(execution);
                    }

                    const liveBlocks = message.id ? blocksRef.current.get(message.id) : undefined;
                    const blocks = liveBlocks ? [...liveBlocks] : [...(message.blocks || [])];
                    if (!blocks.some((block) => block.type === 'command_execution' && block.id === callId)) {
                        const toolBlockIndex = blocks.findIndex((block) => block.type === 'tool_call' && block.id === callId);
                        if (toolBlockIndex >= 0) {
                            blocks.splice(toolBlockIndex + 1, 0, { type: 'command_execution', id: callId });
                        } else {
                            blocks.push({ type: 'command_execution', id: callId });
                        }
                    }

                    const fullContent = accumulatedContentRef.current.id === message.id
                        ? accumulatedContentRef.current.content
                        : message.content;
                    const normalizedBlocks = normalizeSplitBlocks(message, blocks, fullContent);

                    if (message.id) {
                        blocksRef.current.set(message.id, normalizedBlocks.blocks);
                    }

                    nextMessages[messageIndex] = {
                        ...message,
                        tool_calls: (message.tool_calls || []).map((toolCall) => toolCall.id === callId
                            ? {
                                ...toolCall,
                                status: exitCode === 0 ? 'complete' : 'error',
                            }
                            : toolCall),
                        blocks: normalizedBlocks.blocks,
                        content_before_tools: normalizedBlocks.contentBeforeTools,
                        content_after_tools: normalizedBlocks.contentAfterTools,
                        commandExecutions: executions,
                        streaming: message.id
                            ? updateStreamingStateMap(
                                streamingStatesRef.current,
                                message.id,
                                markStreamingToolActivity(message.streaming),
                            )
                            : message.streaming,
                    };
                    return nextMessages;
                });
                applyDeferredRunCommandCompletion(callId);
            });

            unlistenToolCompleted = await listen<ToolExecutionCompletedPayload>('tool-execution-completed', (event) => {
                const { tool_name: toolName, tool_call_id: toolCallId, success, skipped } = event.payload;
                const nextStatus: 'complete' | 'error' | 'skipped' = skipped
                    ? 'skipped'
                    : success
                        ? 'complete'
                        : 'error';

                updateMessages((messages) => messages.map((message) => {
                    if (!message.tool_calls?.some((toolCall) => toolCall.id === toolCallId)) {
                        return message;
                    }
                    const commandExecution = toolName === 'run_command'
                        ? message.commandExecutions?.find((execution) => execution.id === toolCallId)
                        : undefined;
                    const effectiveStatus = !skipped && commandExecution?.exitCode === 0
                        ? 'complete'
                        : nextStatus;
                    return {
                        ...message,
                        tool_calls: (message.tool_calls || []).map((toolCall) => toolCall.id === toolCallId
                            ? {
                                ...toolCall,
                                status: effectiveStatus,
                                ...(skipped && !toolCall.result ? { result: i18n.t('toolCall.skippedByUser', { defaultValue: 'Skipped by user' }) } : {}),
                            }
                            : toolCall),
                    };
                }));

                pendingRunCommandCompletionStatusRef.current.delete(toolCallId);

                if (skipped) {
                    dispatch({
                        type: 'pending-actions/set',
                        actions: pendingActionsRef.current?.filter((action) => action.id !== toolCallId) || null,
                    });
                }
            });

            unlistenTodoUpdated = await listen<{ todos: TodoItem[] }>(EventNames.TODO_UPDATED, (event) => {
                const todos = event.payload.todos;
                dispatch({ type: 'todos/set', todos });
                const allCompleted = todos.length > 0 && todos.every((todo) => todo.status === 'completed');
                if (!allCompleted) {
                    return;
                }
                const timer = window.setTimeout(() => {
                    dispatch({ type: 'todos/set', todos: [] });
                    updateMessages((messages) => [...messages, createPlanSummaryMessage(todos)]);
                }, 1500);
                pendingTimeoutsRef.current.push(timer);
            });

            unlistenBladeEvent = subscribeBladeEventType('Chat', (event) => {
                const chatEvent = event.event.payload as { type: string; payload: any };
                if (chatEvent.type === 'MessageDelta') {
                    if (ignoredChatEventIdsRef.current.has(chatEvent.payload.id)) {
                        return;
                    }
                    if (STREAM_DEBUG_ENABLED) {
                        streamDebugLog('[stream-debug][ui-recv][text]', {
                            id: chatEvent.payload.id,
                            seq: chatEvent.payload.seq,
                            isFinal: chatEvent.payload.is_final,
                            length: chatEvent.payload.chunk.length,
                            preview: streamDebugPreview(chatEvent.payload.chunk),
                        });
                    }
                    messageBufferRef.current?.addMessageDelta(
                        chatEvent.payload.id,
                        chatEvent.payload.seq,
                        chatEvent.payload.chunk,
                        chatEvent.payload.is_final,
                    );
                    return;
                }

                if (chatEvent.type === 'ReasoningDelta') {
                    if (ignoredChatEventIdsRef.current.has(chatEvent.payload.id)) {
                        return;
                    }
                    if (STREAM_DEBUG_ENABLED) {
                        streamDebugLog('[stream-debug][ui-recv][reasoning]', {
                            id: chatEvent.payload.id,
                            seq: chatEvent.payload.seq,
                            isFinal: chatEvent.payload.is_final,
                            length: chatEvent.payload.chunk.length,
                            preview: streamDebugPreview(chatEvent.payload.chunk),
                        });
                    }
                    messageBufferRef.current?.addReasoningDelta(
                        chatEvent.payload.id,
                        chatEvent.payload.seq,
                        chatEvent.payload.chunk,
                        chatEvent.payload.is_final,
                    );
                    return;
                }

                if (chatEvent.type === 'MessageCompleted') {
                    const { id } = chatEvent.payload;
                    if (ignoredChatEventIdsRef.current.has(id)) {
                        return;
                    }
                    flushPendingUpdatesImmediately();
                    const completedAt = Date.now();
                    const previousStreaming = streamingStatesRef.current.get(id)
                        ?? messageByIdRef.current.get(id)?.streaming;
                    if (previousStreaming && !previousStreaming.endTime) {
                        streamingStatesRef.current.set(id, {
                            ...previousStreaming,
                            endTime: completedAt,
                        });
                    }
                    setMessages((messages) => {
                        let changed = false;
                        const nextMessages = messages.map((message) => {
                            if (message.id !== id || !message.streaming || message.streaming.endTime) {
                                return message;
                            }

                            changed = true;
                            return {
                                ...message,
                                streaming: {
                                    ...message.streaming,
                                    endTime: completedAt,
                                },
                            };
                        });
                        return changed ? nextMessages : messages;
                    });
                    scheduleMessageCompletionCleanup(id);
                    toolChunkCountsRef.current.clear();
                    return;
                }

                if (chatEvent.type === 'ToolUpdate') {
                    const { message_id: messageId, tool_call_id: toolCallId, status, result, tool_call: incomingToolCall } = chatEvent.payload;
                    if (ignoredChatEventIdsRef.current.has(messageId) || ignoredChatEventIdsRef.current.has(toolCallId)) {
                        return;
                    }
                    toolChunkCountsRef.current.delete(toolCallId);
                    setToolActivity(null);

                    updateMessages((messages) => {
                        let existingIndex = messageIndexByIdRef.current.get(messageId) ?? -1;
                        if (existingIndex >= messages.length || (existingIndex >= 0 && messages[existingIndex]?.id !== messageId)) {
                            existingIndex = -1;
                        }
                        if (existingIndex === -1) {
                            existingIndex = messages.findIndex((message) => message.id === messageId);
                        }
                        if (existingIndex === -1) {
                            const liveBlocks = blocksRef.current.get(messageId) || [];
                            const nextBlocks = incomingToolCall
                                ? insertToolCallBlockPreservingOrder(liveBlocks, toolCallId)
                                : [...liveBlocks];
                            const accumulatedContent = accumulatedContentRef.current.id === messageId
                                ? accumulatedContentRef.current.content
                                : '';
                            const normalizedBlocks = accumulatedContent && !isWhitespaceOnly(accumulatedContent)
                                ? moveExistingContentAfterTools(nextBlocks, accumulatedContent)
                                : normalizeSplitBlocks(undefined, nextBlocks, accumulatedContent);
                            blocksRef.current.set(messageId, normalizedBlocks.blocks);
                            return insertAssistantMessageAfterLastUser(messages, {
                                id: messageId,
                                role: 'Assistant',
                                content: accumulatedContent,
                                content_before_tools: normalizedBlocks.contentBeforeTools,
                                content_after_tools: normalizedBlocks.contentAfterTools,
                                tool_calls: incomingToolCall ? [{ ...(incomingToolCall as ToolCall), status, result: result ?? undefined }] : [],
                                blocks: normalizedBlocks.blocks,
                                streaming: accumulatedContent
                                    ? updateStreamingStateMap(
                                        streamingStatesRef.current,
                                        messageId,
                                        markStreamingToolActivity(streamingStatesRef.current.get(messageId)),
                                    )
                                    : undefined,
                            });
                        }

                        return messages.map((message) => {
                            if (message.id !== messageId) {
                                return message;
                            }

                            const existingTools = message.tool_calls || [];
                            const toolIndex = existingTools.findIndex((toolCall) => toolCall.id === toolCallId);
                            const liveBlocks = blocksRef.current.get(messageId);
                            const nextBlocks = liveBlocks ? [...liveBlocks] : [...(message.blocks || [])];

                            if (toolIndex >= 0) {
                                const nextTools = [...existingTools];
                                nextTools[toolIndex] = {
                                    ...nextTools[toolIndex],
                                    ...(incomingToolCall || {}),
                                    status,
                                    ...(result ? { result } : {}),
                                };
                                const orderedBlocks = insertToolCallBlockPreservingOrder(nextBlocks, toolCallId);
                                const fullContent = accumulatedContentRef.current.id === messageId
                                    ? accumulatedContentRef.current.content
                                    : message.content;
                                const normalizedBlocks = normalizeSplitBlocks(message, orderedBlocks, fullContent);
                                blocksRef.current.set(messageId, normalizedBlocks.blocks);
                                return {
                                    ...message,
                                    tool_calls: nextTools,
                                    content_before_tools: normalizedBlocks.contentBeforeTools,
                                    content_after_tools: normalizedBlocks.contentAfterTools,
                                    blocks: normalizedBlocks.blocks,
                                    streaming: updateStreamingStateMap(
                                        streamingStatesRef.current,
                                        messageId,
                                        markStreamingToolActivity(message.streaming),
                                    ),
                                };
                            }

                            if (!incomingToolCall) {
                                blocksRef.current.set(messageId, nextBlocks);
                                return {
                                    ...message,
                                    streaming: updateStreamingStateMap(
                                        streamingStatesRef.current,
                                        messageId,
                                        markStreamingToolActivity(message.streaming),
                                    ),
                                };
                            }

                            const orderedBlocks = insertToolCallBlockPreservingOrder(nextBlocks, toolCallId);
                            const accumulatedContent = accumulatedContentRef.current.id === messageId
                                ? accumulatedContentRef.current.content
                                : message.content;
                            const isFirstToolInsertion = existingTools.length === 0
                                && message.content_before_tools === undefined
                                && message.content_after_tools === undefined
                                && !isWhitespaceOnly(accumulatedContent);
                            const normalizedBlocks = isFirstToolInsertion
                                ? moveExistingContentAfterTools(orderedBlocks, accumulatedContent)
                                : normalizeSplitBlocks(
                                    {
                                        ...message,
                                        content_before_tools: message.content_before_tools !== undefined
                                            ? message.content_before_tools
                                            : accumulatedContent,
                                    },
                                    orderedBlocks,
                                    accumulatedContent,
                                );
                            blocksRef.current.set(messageId, normalizedBlocks.blocks);

                            return {
                                ...message,
                                content_before_tools: normalizedBlocks.contentBeforeTools,
                                content_after_tools: normalizedBlocks.contentAfterTools,
                                tool_calls: [
                                    ...existingTools,
                                    {
                                        ...(incomingToolCall as ToolCall),
                                        status,
                                        ...(result ? { result } : {}),
                                    },
                                ],
                                blocks: normalizedBlocks.blocks,
                                streaming: updateStreamingStateMap(
                                    streamingStatesRef.current,
                                    messageId,
                                    markStreamingToolActivity(message.streaming),
                                ),
                            };
                        });
                    });
                    return;
                }

                if (chatEvent.type === 'ToolActivity') {
                    const { tool_name: toolName, file_path: filePath, action, tool_call_id: toolCallId } = chatEvent.payload;
                    const key = toolCallId || `${toolName}:${filePath}`;
                    const now = Date.now();
                    let tracked = toolChunkCountsRef.current.get(key);
                    if (!tracked) {
                        tracked = { chunkCount: 0, startedAt: now, lastChunkAt: now };
                    }
                    if (action === 'streaming') {
                        tracked = {
                            chunkCount: tracked.chunkCount + 1,
                            startedAt: tracked.startedAt,
                            lastChunkAt: now,
                        };
                        toolChunkCountsRef.current.set(key, tracked);
                    } else {
                        tracked = {
                            chunkCount: tracked.chunkCount,
                            startedAt: tracked.startedAt,
                            lastChunkAt: now,
                        };
                    }

                    setToolActivity({
                        toolName,
                        filePath,
                        action,
                        toolCallId,
                        chunkCount: tracked.chunkCount,
                        startedAt: tracked.startedAt,
                        lastChunkAt: tracked.lastChunkAt,
                    });
                    if (action === 'streaming' && toolCallId) {
                        updateMessages((messages) => {
                            let changed = false;
                            const nextMessages = messages.map((message) => {
                                if (
                                    message.id
                                    && message.streaming
                                    && message.streaming.activeKind !== 'tool'
                                    && message.tool_calls?.some((toolCall) => toolCall.id === toolCallId)
                                ) {
                                    const streaming = updateStreamingStateMap(
                                        streamingStatesRef.current,
                                        message.id,
                                        markStreamingToolActivity(message.streaming),
                                    );
                                    changed = true;
                                    return { ...message, streaming };
                                }
                                return message;
                            });

                            return changed ? nextMessages : messages;
                        });
                    }
                    dispatch({
                        type: 'chat-activity/upsert',
                        activity: {
                            id: key,
                            kind: 'tool',
                            toolName,
                            action,
                            ...(toolCallId ? { toolCallId } : {}),
                            ...(filePath ? { filePath } : {}),
                            detail: filePath || action,
                            status: action === 'streaming' ? 'executing' : 'complete',
                            startedAt: tracked.startedAt,
                            updatedAt: tracked.lastChunkAt,
                        },
                    });

                    if (action !== 'streaming') {
                        toolChunkCountsRef.current.delete(key);
                        const timer = window.setTimeout(() => {
                            const current = toolActivityRef.current;
                            if (!current) {
                                return;
                            }
                            const currentKey = current.toolCallId || `${current.toolName}:${current.filePath}`;
                            if (currentKey === key) {
                                setToolActivity(null);
                            }
                        }, 2000);
                        pendingTimeoutsRef.current.push(timer);
                    }
                    return;
                }

                if (chatEvent.type === 'CognitiveInterrupt') {
                    const interrupt = chatEvent.payload as CognitiveInterruptState;
                    const now = Date.now();
                    const activityId = `cognitive-interrupt:${interrupt.frame || 'default'}`;

                    dispatch({
                        type: 'chat-activity/upsert',
                        activity: {
                            id: activityId,
                            kind: 'cognitive_interrupt',
                            interrupt,
                            detail: cognitiveInterruptActivityDetail(interrupt),
                            status: cognitiveInterruptStatus(interrupt.state),
                            startedAt: now,
                            updatedAt: now,
                        },
                    });

                    dispatch({ type: 'cognitive-interrupt/set', interrupt });

                    if (interrupt.state === 'cleared') {
                        const timer = window.setTimeout(() => {
                            if (cognitiveInterruptRef.current === interrupt) {
                                dispatch({ type: 'cognitive-interrupt/set', interrupt: null });
                            }
                        }, COGNITIVE_INTERRUPT_CLEAR_DELAY_MS);
                        pendingTimeoutsRef.current.push(timer);
                    }
                    return;
                }
            });
        };

        void setupListeners();

        return () => {
            unlistenDone?.();
            unlistenError?.();
            unlistenContextLength?.();
            unlistenMessageTooLarge?.();
            unlistenPermission?.();
            unlistenApprovalRequest?.();
            unlistenCommand?.();
            unlistenToolCompleted?.();
            unlistenTodoUpdated?.();
            unlistenBladeEvent?.();
            if (flushFrameRef.current !== null) {
                window.cancelAnimationFrame(flushFrameRef.current);
                flushFrameRef.current = null;
            }
            clearPendingTimers();
        };
    }, [applyDeferredRunCommandCompletion, clearPendingTimers, flushPendingUpdates, flushPendingUpdatesImmediately, queueMessageUpdate, setMessages, setToolActivity, updateMessages, updateToolCallsStatusLocally]);

    const dispatchToBackend = useCallback(async (text: string, attachments?: ImageAttachment[], mentions?: ComposerMention[], mode?: ChatMode): Promise<boolean> => {
        try {
            dispatchInFlightRef.current = true;
            dispatch({ type: 'loading/set', loading: true });
            dispatch({ type: 'error/set', error: null });

            const context = await requestFreshEditorContext();
            await BladeDispatcher.chat({
                type: 'SendMessage',
                payload: {
                    content: text,
                    model: selectedModelIdRef.current,
                    images: attachments?.map((attachment) => ({
                        data: attachment.data,
                        mime_type: attachment.mime_type,
                        name: attachment.name,
                        size: attachment.size,
                    })),
                    context,
                    mentions: toChatMentions(mentions),
                    mode: mode ?? chatModeRef.current,
                },
            });
            firstDispatchRef.current = false;
            return true;
        } catch (error) {
            console.error('[useChatV2] Failed to send message:', error);
            dispatchInFlightRef.current = false;
            dispatch({ type: 'error/set', error: error instanceof Error ? error.message : String(error) });
            dispatch({ type: 'loading/set', loading: false });
            return false;
        }
    }, [requestFreshEditorContext, toChatMentions]);

    useEffect(() => {
        if (state.loading || dispatchInFlightRef.current || state.messageQueue.length === 0) {
            return;
        }
        const nextMessage = state.messageQueue[0];
        if (blockedQueuedRequestRef.current === nextMessage) {
            return;
        }
        dispatchInFlightRef.current = true;
        void (async () => {
            // Add user message to UI before dispatching from queue
            const userMessage: ChatMessage = {
                id: crypto.randomUUID(),
                role: 'User',
                content: nextMessage.text,
                images: nextMessage.attachments,
                mentions: nextMessage.mentions,
            };
            updateMessages((messages) => [...messages, userMessage]);

            const accepted = await dispatchToBackend(nextMessage.text, nextMessage.attachments, nextMessage.mentions, nextMessage.mode);
            if (accepted) {
                blockedQueuedRequestRef.current = null;
                dispatch({ type: 'queue/shift' });
            } else {
                blockedQueuedRequestRef.current = nextMessage;
            }
        })();
    }, [dispatchToBackend, state.loading, state.messageQueue, updateMessages]);

    const sendMessage = useCallback((text: string, attachments?: ImageAttachment[], mentions?: ComposerMention[], mode?: ChatMode) => {
        const requestMode = mode ?? chatModeRef.current;
        const request = { text, attachments, mentions, mode: requestMode };

        if (!loadingRef.current && !dispatchInFlightRef.current && messageQueueRef.current.length === 0) {
            const userMessage: ChatMessage = {
                id: crypto.randomUUID(),
                role: 'User',
                content: request.text,
                images: request.attachments,
                mentions: request.mentions,
            };
            updateMessages((messages) => [...messages, userMessage]);
            void dispatchToBackend(request.text, request.attachments, request.mentions, request.mode);
            return;
        }

        blockedQueuedRequestRef.current = null;
        dispatch({ type: 'queue/enqueue', request });
    }, [dispatchToBackend, updateMessages]);

    const setChatMode = useCallback((mode: ChatMode) => {
        dispatch({ type: 'mode/set', mode });
    }, []);

    const deleteQueuedRequest = useCallback((index: number) => {
        dispatch({ type: 'queue/delete', index });
    }, []);

    const editLastUserMessage = useCallback(async (): Promise<QueuedRequest | null> => {
        if (state.loading || dispatchInFlightRef.current) {
            try {
                await BladeDispatcher.chat({ type: 'StopGeneration', payload: {} });
            } catch (error) {
                console.error('[useChatV2] Failed to stop generation before message edit:', error);
            }
            dispatchInFlightRef.current = false;
            dispatch({ type: 'loading/set', loading: false });
        }

        const messages = messagesRef.current;
        const userIndex = [...messages].reverse().findIndex((message) => message.role === 'User');
        if (userIndex === -1) {
            return null;
        }

        const messageIndex = messages.length - 1 - userIndex;
        const message = messages[messageIndex];
        const request: QueuedRequest = {
            text: message.content,
            attachments: message.images?.filter(isImageAttachment),
            mentions: message.mentions,
            mode: chatModeRef.current,
        };

        try {
            await invoke('truncate_conversation', { len: messageIndex, resetSession: true });
        } catch (error) {
            console.error('[useChatV2] Failed to truncate conversation for message edit:', error);
            dispatch({ type: 'error/set', error: error instanceof Error ? error.message : String(error) });
            return null;
        }

        messages.slice(messageIndex).forEach((removedMessage) => {
            if (removedMessage.id) {
                ignoredChatEventIdsRef.current.add(removedMessage.id);
            }
            removedMessage.tool_calls?.forEach((toolCall) => {
                ignoredChatEventIdsRef.current.add(toolCall.id);
            });
        });
        resetStreamingState();
        dispatch({ type: 'messages/replace', messages: messages.slice(0, messageIndex) });
        dispatch({ type: 'chat-activity/clear' });
        dispatch({ type: 'cognitive-interrupt/set', interrupt: null });
        dispatch({ type: 'todos/set', todos: [] });
        dispatch({ type: 'queue/clear' });
        dispatch({ type: 'pending-actions/set', actions: null });
        dispatch({ type: 'pending-approval-request/set', request: null });
        dispatch({ type: 'waiting-for-approval/set', waiting: false });
        return request;
    }, [resetStreamingState, state.loading]);

    const stopGeneration = useCallback(async () => {
        const inFlightToolCallIds = Array.from(new Set([
            ...messagesRef.current.flatMap((message) => (message.tool_calls || [])
                .filter((toolCall) => toolCall.status === undefined || toolCall.status === 'pending' || toolCall.status === 'executing')
                .map((toolCall) => toolCall.id)),
            ...(pendingActionsRef.current?.map((action) => action.id) || []),
        ]));

        if (inFlightToolCallIds.length > 0) {
            markToolCallsSkippedLocally(inFlightToolCallIds, i18n.t('toolCall.stoppedByUser', { defaultValue: 'Stopped by user' }));
        }

        toolChunkCountsRef.current.clear();
        setToolActivity(null);
        blockedQueuedRequestRef.current = null;
        dispatch({ type: 'cognitive-interrupt/set', interrupt: null });
        dispatch({ type: 'pending-actions/set', actions: null });
        dispatch({ type: 'pending-approval-request/set', request: null });
        dispatch({ type: 'waiting-for-approval/set', waiting: false });

        try {
            await BladeDispatcher.chat({ type: 'StopGeneration', payload: {} });
            dispatchInFlightRef.current = false;
            dispatch({ type: 'loading/set', loading: false });
        } catch (error) {
            console.error('[useChatV2] Failed to stop generation:', error);
        }
    }, [markToolCallsSkippedLocally, setToolActivity]);

    const approveTool = useCallback(async (approved: boolean) => {
        try {
            await BladeDispatcher.workflow({
                type: 'ApproveTool',
                payload: { approved },
            });
        } catch (error) {
            console.error('[useChatV2] Failed to approve tool:', error);
        }
    }, []);

    const approveToolDecision = useCallback(async (decision: string) => {
        try {
            const pendingIds = pendingActionsRef.current?.map((action) => action.id) || [];
            if (decision === 'reject' && pendingIds.length > 0) {
                markToolCallsSkippedLocally(pendingIds);
            }
            dispatch({ type: 'pending-actions/set', actions: null });
            dispatch({ type: 'waiting-for-approval/set', waiting: false });
            await BladeDispatcher.workflow({
                type: 'ApproveToolDecision',
                payload: { decision },
            });
        } catch (error) {
            console.error('[useChatV2] Failed to approve tool decision:', error);
        }
    }, [markToolCallsSkippedLocally]);

    const respondToApprovalRequest = useCallback(async (approved: boolean) => {
        const request = pendingApprovalRequestRef.current;
        if (!request) {
            return;
        }

        if (approved) {
            markToolCallsExecutingLocally([request.toolCallId]);
        } else {
            markToolCallsSkippedLocally([request.toolCallId], i18n.t('toolCall.deniedByUser', { defaultValue: 'Denied by user' }));
        }

        dispatch({ type: 'pending-approval-request/set', request: null });
        dispatch({ type: 'waiting-for-approval/set', waiting: false });

        try {
            await invoke('send_approval_response', {
                sessionId: request.sessionId,
                approvalId: request.approvalId,
                toolCallId: request.toolCallId,
                approved,
            });
        } catch (error) {
            console.error('[useChatV2] Failed to send approval response:', error);
        }
    }, [markToolCallsExecutingLocally, markToolCallsSkippedLocally]);

    const skipSingleCommand = useCallback(async (callId: string) => {
        markToolCallsSkippedLocally([callId]);
        dispatch({
            type: 'pending-actions/set',
            actions: pendingActionsRef.current?.filter((action) => action.id !== callId) || null,
        });
        try {
            await invoke('approve_single_command', { callId, approved: false });
        } catch (error) {
            console.error('[useChatV2] Failed to skip single command:', error);
        }
    }, [markToolCallsSkippedLocally]);

    const approveSingleCommand = useCallback(async (callId: string) => {
        markToolCallsExecutingLocally([callId]);
        dispatch({
            type: 'pending-actions/set',
            actions: pendingActionsRef.current?.filter((action) => action.id !== callId) || null,
        });
        try {
            await invoke('approve_single_command', { callId, approved: true });
        } catch (error) {
            console.error('[useChatV2] Failed to approve single command:', error);
        }
    }, [markToolCallsExecutingLocally]);

    const newConversation = useCallback(async () => {
        try {
            await BladeDispatcher.chat({
                type: 'NewConversation',
                payload: { model: selectedModelIdRef.current },
            });
            resetStreamingState();
            firstDispatchRef.current = true;
            dispatch({ type: 'messages/replace', messages: [] });
            dispatch({ type: 'chat-activity/clear' });
            dispatch({ type: 'cognitive-interrupt/set', interrupt: null });
            dispatch({ type: 'todos/set', todos: [] });
            dispatch({ type: 'queue/clear' });
            dispatch({ type: 'pending-actions/set', actions: null });
            dispatch({ type: 'pending-approval-request/set', request: null });
            dispatch({ type: 'waiting-for-approval/set', waiting: false });
        } catch (error) {
            console.error('[useChatV2] Failed to start new conversation:', error);
        }
    }, [resetStreamingState]);

    const undoTool = useCallback(async (toolCallId: string) => {
        try {
            await invoke<string[]>('undo_batch', { groupId: toolCallId });
        } catch (error) {
            console.error('[useChatV2] Failed to undo tool batch:', error);
        }
    }, []);

    const loadConversation = useCallback((messages: ChatMessage[]) => {
        resetStreamingState();
        firstDispatchRef.current = true;
        replaceMessagesPreservingImagePreviews(messages);
        dispatch({ type: 'chat-activity/clear' });
        dispatch({ type: 'cognitive-interrupt/set', interrupt: null });
        dispatch({ type: 'todos/set', todos: [] });
        dispatch({ type: 'queue/clear' });
        dispatch({ type: 'pending-actions/set', actions: null });
        dispatch({ type: 'pending-approval-request/set', request: null });
        dispatch({ type: 'waiting-for-approval/set', waiting: false });
    }, [replaceMessagesPreservingImagePreviews, resetStreamingState]);

    const setConversation = useCallback((messages: ChatMessage[] | ((current: ChatMessage[]) => ChatMessage[])) => {
        if (typeof messages === 'function') {
            setMessages(messages);
            return;
        }
        replaceMessagesPreservingImagePreviews(messages);
    }, [replaceMessagesPreservingImagePreviews, setMessages]);

    const setActiveTodos = useCallback((value: TodoItem[] | ((current: TodoItem[]) => TodoItem[])) => {
        if (typeof value === 'function') {
            dispatch({ type: 'todos/set', todos: value(activeTodosRef.current) });
            return;
        }
        dispatch({ type: 'todos/set', todos: value });
    }, []);

    // Hide local models the user has unchecked in Settings -> Local AI (cloud models
    // and the currently-selected model are always kept visible).
    const visibleModels = useMemo(() => {
        if (hiddenLocalModels.length === 0) {
            return state.models;
        }
        const hidden = new Set(hiddenLocalModels);
        return state.models.filter((model) => {
            const isLocal = model.provider === 'ollama' || model.provider === 'openai-compat';
            if (!isLocal) {
                return true;
            }
            if (model.id === state.selectedModelId) {
                return true;
            }
            return !hidden.has(model.id);
        });
    }, [state.models, hiddenLocalModels, state.selectedModelId]);

    return {
        messages: state.messages,
        loading: state.loading,
        error: state.error,
        sendMessage,
        stopGeneration,
        models: visibleModels,
        refreshModels,
        selectedModelId: state.selectedModelId,
        setSelectedModelId,
        chatMode: state.chatMode,
        setChatMode,
        pendingActions: state.pendingActions,
        pendingApprovalRequest: state.pendingApprovalRequest,
        waitingForApproval: state.waitingForApproval,
        approveTool,
        approveToolDecision,
        respondToApprovalRequest,
        approveSingleCommand,
        skipSingleCommand,
        newConversation,
        undoTool,
        setConversation,
        loadConversation,
        toolActivity: state.toolActivity,
        cognitiveInterrupt: state.cognitiveInterrupt,
        chatActivities: state.chatActivities,
        activeTodos: state.activeTodos,
        setActiveTodos,
        messageQueue: state.messageQueue,
        deleteQueuedRequest,
        editLastUserMessage,
    };
}
