import { useCallback, useEffect, useReducer, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import { EditorFacade } from '../services/editorFacade';
import { useEditorState } from '../contexts/EditorContext';
import { MessageBuffer } from '../utils/eventBuffer';
import { ensureMessagesHaveBlocks } from '../utils/messageBlocks';
import { EventNames, type RequestConfirmationPayload, type StructuredAction, type ToolExecutionCompletedPayload, type TodoItem } from '../types/events';
import type { BladeEventEnvelope, ChatMention } from '../types/blade';
import type { ChatImage, ChatMessage, ChatMode, ComposerMention, CommandExecution, ImageAttachment, MessageBlock, ModelInfo, QueuedRequest, StreamingState, ToolActivityState, ToolCall } from '../types/chat';

const FLUSH_INTERVAL_MS = 80;
const TOOL_ACTIVITY_DISPATCH_INTERVAL_MS = 120;
const MESSAGE_COMPLETION_GRACE_MS = 1200;

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

function streamDebugPreview(value: string): string {
    const normalized = value.replace(/\n/g, '\\n').replace(/\r/g, '\\r');
    return normalized.length > 160 ? `${normalized.slice(0, 160)}…` : normalized;
}

function streamDebugLog(tag: string, payload: Record<string, unknown>): void {
    const message = `${tag} ${JSON.stringify(payload)}`;
    console.debug(message);
    void invoke('log_frontend', { message }).catch(() => undefined);
}

type ChatState = {
    messages: ChatMessage[];
    loading: boolean;
    error: string | null;
    models: ModelInfo[];
    selectedModelId: string;
    chatMode: ChatMode;
    pendingActions: StructuredAction[] | null;
    toolActivity: ToolActivityState | null;
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
    | { type: 'tool-activity/set'; activity: ToolActivityState | null }
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
    toolActivity: null,
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
        case 'tool-activity/set':
            if (areToolActivitiesEqual(state.toolActivity, action.activity)) {
                return state;
            }
            return { ...state, toolActivity: action.activity };
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

export function useChatV2() {
    const editorState = useEditorState();
    const [state, dispatch] = useReducer(chatReducer, initialState);

    const editorStateRef = useRef(editorState);
    const firstDispatchRef = useRef(true);
    const messagesRef = useRef<ChatMessage[]>([]);
    const messageByIdRef = useRef<Map<string, ChatMessage>>(new Map());
    const messageIndexByIdRef = useRef<Map<string, number>>(new Map());
    const toolCallOwnerMessageIdRef = useRef<Map<string, string>>(new Map());
    const pendingActionsRef = useRef<StructuredAction[] | null>(null);
    const selectedModelIdRef = useRef(state.selectedModelId);
    const chatModeRef = useRef(state.chatMode);
    const activeTodosRef = useRef<TodoItem[]>(state.activeTodos);
    const toolActivityRef = useRef<ToolActivityState | null>(state.toolActivity);
    const hasExplicitModelRef = useRef(false);
    const blocksRef = useRef<Map<string, MessageBlock[]>>(new Map());
    const messageBufferRef = useRef<MessageBuffer | null>(null);
    const accumulatedContentRef = useRef<{ id: string; content: string }>({ id: '', content: '' });
    const accumulatedReasoningRef = useRef<{ id: string; content: string }>({ id: '', content: '' });
    const dispatchInFlightRef = useRef(false);
    const pendingUpdatesRef = useRef<Map<string, { content: string; reasoning: string; blocks: MessageBlock[]; streaming?: StreamingState }>>(new Map());
    const flushScheduledRef = useRef<number | null>(null);
    const streamingStatesRef = useRef<Map<string, StreamingState>>(new Map());
    const toolChunkCountsRef = useRef<Map<string, { chunkCount: number; startedAt: number; lastChunkAt: number }>>(new Map());
    const messageCompletionCleanupTimersRef = useRef<Map<string, number>>(new Map());

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
            if (accumulatedContentRef.current.id === id) {
                accumulatedContentRef.current = { id: '', content: '' };
            }
            if (accumulatedReasoningRef.current.id === id) {
                accumulatedReasoningRef.current = { id: '', content: '' };
            }
            toolActivityRef.current = null;
            lastToolActivityDispatchAtRef.current = Date.now();
            dispatch({ type: 'tool-activity/set', activity: null });
        }, MESSAGE_COMPLETION_GRACE_MS);
        messageCompletionCleanupTimersRef.current.set(id, timerId);
    }, [cancelMessageCompletionCleanup]);

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
        editorStateRef.current = editorState;
    }, [editorState]);

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
        selectedModelIdRef.current = state.selectedModelId;
    }, [state.selectedModelId]);

    useEffect(() => {
        chatModeRef.current = state.chatMode;
    }, [state.chatMode]);

    useEffect(() => {
        activeTodosRef.current = state.activeTodos;
    }, [state.activeTodos]);

    useEffect(() => {
        toolActivityRef.current = state.toolActivity;
    }, [state.toolActivity]);

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

        return {
            active_file: safeActiveFile,
            open_files: normalizedOpenFiles,
            cursor_line: snapshot.cursorLine ?? null,
            cursor_column: snapshot.cursorColumn ?? null,
            selection_start: snapshot.selectionStartLine ?? null,
            selection_end: snapshot.selectionEndLine ?? null,
        };
    }, []);

    const requestFreshEditorContext = useCallback(async () => {
        if (!firstDispatchRef.current) {
            return buildEditorContext(editorStateRef.current);
        }

        let unlisten: (() => void) | undefined;
        try {
            const snapshotPromise = new Promise<{
                active_file: string | null;
                open_files: string[];
                cursor_line: number | null;
                cursor_column: number | null;
                selection_start: number | null;
                selection_end: number | null;
            }>((resolve, reject) => {
                const timeout = window.setTimeout(() => {
                    if (unlisten) {
                        unlisten();
                        unlisten = undefined;
                    }
                    reject(new Error('Editor state snapshot timeout'));
                }, 300);

                listen<BladeEventEnvelope>('blade-event', (event) => {
                    const bladeEvent = event.payload.event;
                    if (bladeEvent.type !== 'Editor') {
                        return;
                    }
                    const payload = bladeEvent.payload as { type: string; payload: any };
                    if (payload.type !== 'StateSnapshot') {
                        return;
                    }

                    window.clearTimeout(timeout);
                    if (unlisten) {
                        unlisten();
                        unlisten = undefined;
                    }
                    resolve(payload.payload);
                }).then((fn) => {
                    unlisten = fn;
                }).catch((error) => {
                    window.clearTimeout(timeout);
                    reject(error);
                });
            });

            await EditorFacade.getState();
            const snapshot = await snapshotPromise;
            return buildEditorContext({
                activeFile: snapshot.active_file,
                openFiles: snapshot.open_files,
                cursorLine: snapshot.cursor_line,
                cursorColumn: snapshot.cursor_column,
                selectionStartLine: snapshot.selection_start,
                selectionEndLine: snapshot.selection_end,
            });
        } catch {
            return buildEditorContext(editorStateRef.current);
        } finally {
            unlisten?.();
        }
    }, [buildEditorContext]);

    const refreshModels = useCallback(async () => {
        const models = await invoke<ModelInfo[]>('list_models');
        dispatch({ type: 'models/set', models });
        return models;
    }, []);

    const resetStreamingState = useCallback(() => {
        messageBufferRef.current?.clearAll();
        accumulatedContentRef.current = { id: '', content: '' };
        accumulatedReasoningRef.current = { id: '', content: '' };
        blocksRef.current.clear();
        pendingUpdatesRef.current.clear();
        streamingStatesRef.current.clear();
        toolChunkCountsRef.current.clear();
        if (flushScheduledRef.current !== null) {
            window.clearTimeout(flushScheduledRef.current);
            flushScheduledRef.current = null;
        }
        clearPendingTimers();
        toolActivityRef.current = null;
        lastToolActivityDispatchAtRef.current = Date.now();
        dispatchInFlightRef.current = false;
        dispatch({ type: 'loading/set', loading: false });
        dispatch({ type: 'pending-actions/set', actions: null });
    }, [clearPendingTimers]);

    const flushPendingUpdates = useCallback(() => {
        flushScheduledRef.current = null;
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
                        || (existingMessage.streaming?.endTime ?? null) !== (update.streaming?.endTime ?? null);

                    if (
                        existingMessage.content !== update.content
                        || existingMessage.reasoning !== update.reasoning
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
                    blocks: update.blocks,
                    streaming: update.streaming,
                });
            });

            pending.clear();
            return changed ? nextMessages : previousMessages;
        });
    }, [setMessages]);

    const scheduleFlush = useCallback(() => {
        if (flushScheduledRef.current === null) {
            flushScheduledRef.current = window.setTimeout(flushPendingUpdates, FLUSH_INTERVAL_MS);
        }
    }, [flushPendingUpdates]);

    const queueMessageUpdate = useCallback((
        id: string,
        content: string,
        reasoning: string,
        blocks: MessageBlock[],
        streaming?: StreamingState,
    ) => {
        pendingUpdatesRef.current.set(id, { content, reasoning, blocks, streaming });
        scheduleFlush();
    }, [scheduleFlush]);

    const setSelectedModelId = useCallback(async (modelId: string) => {
        hasExplicitModelRef.current = true;
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

    const updateMessages = useCallback((updater: (messages: ChatMessage[]) => ChatMessage[]) => {
        setMessages(updater);
    }, [setMessages]);

    const updateToolCallsStatusLocally = useCallback((
        toolCallIds: string[],
        nextStatus: 'pending' | 'executing' | 'complete' | 'error' | 'skipped',
        fallbackResultText: string,
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
                        ...(toolCall.result ? {} : { result: fallbackResultText }),
                    }
                    : toolCall),
            };
        }));
    }, [updateMessages]);

    const markToolCallsSkippedLocally = useCallback((toolCallIds: string[], skippedResultText = 'Skipped by user') => {
        updateToolCallsStatusLocally(toolCallIds, 'skipped', skippedResultText);
    }, [updateToolCallsStatusLocally]);

    useEffect(() => {
        async function init() {
            try {
                if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
                    return;
                }

                const [history, modelList, isStreaming] = await Promise.all([
                    invoke<ChatMessage[]>('get_conversation'),
                    invoke<ModelInfo[]>('list_models'),
                    invoke<boolean>('get_chat_status'),
                ]);

                replaceMessagesPreservingImagePreviews(ensureMessagesHaveBlocks(history));
                dispatch({ type: 'models/set', models: modelList });

                if (isStreaming) {
                    dispatch({ type: 'loading/set', loading: true });
                }

                if (modelList.length > 0 && !hasExplicitModelRef.current) {
                    const defaultModel = modelList.find((model) => model.id === 'anthropic/claude-sonnet-4-5-20250929')
                        || modelList.find((model) => model.id === 'openai/gpt-5.2')
                        || modelList[0];
                    dispatch({ type: 'model/set', modelId: defaultModel.id });
                }
            } catch (error) {
                console.error('[useChatV2] Failed to initialize chat:', error);
            }
        }

        void init();
    }, []);

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
                    const streaming: StreamingState = {
                        seq: Math.max(seq, previousStreaming?.seq ?? 0),
                        startTime: previousStreaming?.startTime ?? now,
                        lastSeqAt: now,
                    };
                    streamingStatesRef.current.set(id, streaming);

                    // Delta-mode append: the backend sends true deltas sequenced
                    // via EventBuffer, so we simply append each chunk.
                    if (type === 'reasoning') {
                        if (accumulatedReasoningRef.current.id !== id) {
                            accumulatedReasoningRef.current = { id, content: '' };
                        }
                        accumulatedReasoningRef.current.content += chunk;
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
                    } else {
                        if (accumulatedContentRef.current.id !== id) {
                            accumulatedContentRef.current = { id, content: '' };
                        }
                        accumulatedContentRef.current.content += chunk;
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
                        } else {
                            blocks = [...blocks, { type: 'reasoning', content: chunk, id: crypto.randomUUID() }];
                        }
                    } else {
                        const fullContent = accumulatedContentRef.current.content;
                        // Defer creating a text block until we have non-whitespace content
                        const hasExistingTextBlock = blocks.some((block) => block.type === 'text');
                        if (!hasExistingTextBlock && isWhitespaceOnly(fullContent)) {
                            blocksRef.current.set(id, blocks);
                            queueMessageUpdate(
                                id,
                                accumulatedContentRef.current.id === id ? accumulatedContentRef.current.content : '',
                                accumulatedReasoningRef.current.id === id ? accumulatedReasoningRef.current.content : '',
                                blocks,
                                streaming,
                            );
                            return;
                        }
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

                    blocksRef.current.set(id, blocks);
                    queueMessageUpdate(
                        id,
                        accumulatedContentRef.current.id === id ? accumulatedContentRef.current.content : '',
                        accumulatedReasoningRef.current.id === id ? accumulatedReasoningRef.current.content : '',
                        blocks,
                        streaming,
                    );
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
                    queueMessageUpdate(
                        id,
                        accumulatedContentRef.current.id === id ? accumulatedContentRef.current.content : (existingMessage?.content || ''),
                        accumulatedReasoningRef.current.id === id ? accumulatedReasoningRef.current.content : (existingMessage?.reasoning || ''),
                        blocks,
                        streaming,
                    );
                    flushPendingUpdates();
                    blocksRef.current.delete(id);
                },
            );
        }

        let unlistenDone: (() => void) | undefined;
        let unlistenError: (() => void) | undefined;
        let unlistenContextLength: (() => void) | undefined;
        let unlistenMessageTooLarge: (() => void) | undefined;
        let unlistenPermission: (() => void) | undefined;
        let unlistenCommand: (() => void) | undefined;
        let unlistenToolCompleted: (() => void) | undefined;
        let unlistenTodoUpdated: (() => void) | undefined;
        let unlistenBladeEvent: (() => void) | undefined;

        const setupListeners = async () => {
            unlistenDone = await listen('chat-done', () => {
                const inFlightToolCallIds = Array.from(new Set(
                    messagesRef.current.flatMap((message) => (message.tool_calls || [])
                        .filter((toolCall) => toolCall.status === undefined || toolCall.status === 'pending' || toolCall.status === 'executing')
                        .map((toolCall) => toolCall.id)),
                ));

                if (inFlightToolCallIds.length > 0) {
                    updateToolCallsStatusLocally(inFlightToolCallIds, 'complete', 'Completed after conversation ended');
                }

                dispatchInFlightRef.current = false;
                dispatch({ type: 'loading/set', loading: false });
                dispatch({ type: 'pending-actions/set', actions: null });

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

            unlistenError = await listen<string>('chat-error', (event) => {
                const inFlightToolCallIds = Array.from(new Set(
                    messagesRef.current.flatMap((message) => (message.tool_calls || [])
                        .filter((toolCall) => toolCall.status === undefined || toolCall.status === 'pending' || toolCall.status === 'executing')
                        .map((toolCall) => toolCall.id)),
                ));
                if (inFlightToolCallIds.length > 0) {
                    updateToolCallsStatusLocally(inFlightToolCallIds, 'error', event.payload);
                }
                dispatchInFlightRef.current = false;
                dispatch({ type: 'loading/set', loading: false });
                dispatch({ type: 'pending-actions/set', actions: null });
                dispatch({ type: 'error/set', error: event.payload });
            });

            unlistenContextLength = await listen<{
                message: string;
                token_count: number | null;
                max_tokens: number | null;
                excess: number | null;
                recoverable: boolean;
                recovery_hint: string | null;
            }>('context-length-exceeded', (event) => {
                const { message, token_count, max_tokens, recoverable, recovery_hint } = event.payload;
                const tokenInfo = token_count && max_tokens ? ` (${token_count.toLocaleString()} / ${max_tokens.toLocaleString()} tokens)` : '';
                dispatchInFlightRef.current = false;
                dispatch({ type: 'loading/set', loading: false });
                dispatch({ type: 'pending-actions/set', actions: null });
                updateMessages((messages) => [
                    ...messages,
                    buildSystemAssistantMessage(
                        `system-context-${Date.now()}`,
                        `⚠️ **Context Limit Reached**${tokenInfo}\n\n${message}\n\n${recoverable
                            ? (recovery_hint || 'The AI is attempting to recover automatically. You can also try:\n- Starting a new conversation\n- Asking the AI to summarize the conversation')
                            : 'Please start a new conversation to continue.'}`,
                    ),
                ]);
            });

            unlistenMessageTooLarge = await listen<{
                message: string;
                recovery_hint: string;
            }>('message-too-large', (event) => {
                dispatchInFlightRef.current = false;
                dispatch({ type: 'loading/set', loading: false });
                updateMessages((messages) => [
                    ...messages,
                    buildSystemAssistantMessage(
                        `system-size-${Date.now()}`,
                        `⚠️ **Response Too Large**\n\n${event.payload.message}\n\n**Recovery hint:** ${event.payload.recovery_hint}`,
                    ),
                ]);
            });

            unlistenPermission = await listen<RequestConfirmationPayload>('request-confirmation', (event) => {
                dispatch({ type: 'pending-actions/set', actions: event.payload.actions });
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

                    if (message.id) {
                        blocksRef.current.set(message.id, blocks);
                    }

                    nextMessages[messageIndex] = {
                        ...message,
                        blocks,
                        commandExecutions: executions,
                    };
                    return nextMessages;
                });
            });

            unlistenToolCompleted = await listen<ToolExecutionCompletedPayload>('tool-execution-completed', (event) => {
                const { tool_call_id: toolCallId, success, skipped } = event.payload;
                const nextStatus: 'complete' | 'error' | 'skipped' = skipped
                    ? 'skipped'
                    : success
                        ? 'complete'
                        : 'error';

                updateMessages((messages) => messages.map((message) => {
                    if (!message.tool_calls?.some((toolCall) => toolCall.id === toolCallId)) {
                        return message;
                    }
                    return {
                        ...message,
                        tool_calls: (message.tool_calls || []).map((toolCall) => toolCall.id === toolCallId
                            ? {
                                ...toolCall,
                                status: nextStatus,
                                ...(skipped && !toolCall.result ? { result: 'Skipped by user' } : {}),
                            }
                            : toolCall),
                    };
                }));

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

            unlistenBladeEvent = await listen<BladeEventEnvelope>('blade-event', (event) => {
                const envelope = event.payload;
                if (envelope.event.type !== 'Chat') {
                    return;
                }

                const chatEvent = envelope.event.payload as { type: string; payload: any };
                if (chatEvent.type === 'MessageDelta') {
                    streamDebugLog('[stream-debug][ui-recv][text]', {
                        id: chatEvent.payload.id,
                        seq: chatEvent.payload.seq,
                        isFinal: chatEvent.payload.is_final,
                        length: chatEvent.payload.chunk.length,
                        preview: streamDebugPreview(chatEvent.payload.chunk),
                    });
                    messageBufferRef.current?.addMessageDelta(
                        chatEvent.payload.id,
                        chatEvent.payload.seq,
                        chatEvent.payload.chunk,
                        chatEvent.payload.is_final,
                    );
                    return;
                }

                if (chatEvent.type === 'ReasoningDelta') {
                    streamDebugLog('[stream-debug][ui-recv][reasoning]', {
                        id: chatEvent.payload.id,
                        seq: chatEvent.payload.seq,
                        isFinal: chatEvent.payload.is_final,
                        length: chatEvent.payload.chunk.length,
                        preview: streamDebugPreview(chatEvent.payload.chunk),
                    });
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
                    scheduleMessageCompletionCleanup(id);
                    toolChunkCountsRef.current.clear();
                    return;
                }

                if (chatEvent.type === 'ToolUpdate') {
                    const { message_id: messageId, tool_call_id: toolCallId, status, result, tool_call: incomingToolCall } = chatEvent.payload;
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
                            const nextBlocks = [...liveBlocks];
                            if (incomingToolCall && !nextBlocks.some((block) => block.type === 'tool_call' && block.id === toolCallId)) {
                                nextBlocks.push({ type: 'tool_call', id: toolCallId });
                            }
                            blocksRef.current.set(messageId, nextBlocks);
                            return [
                                ...messages,
                                {
                                    id: messageId,
                                    role: 'Assistant',
                                    content: '',
                                    tool_calls: incomingToolCall ? [{ ...(incomingToolCall as ToolCall), status, result: result ?? undefined }] : [],
                                    blocks: nextBlocks,
                                },
                            ];
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
                                blocksRef.current.set(messageId, nextBlocks);
                                return { ...message, tool_calls: nextTools, blocks: nextBlocks };
                            }

                            if (!incomingToolCall) {
                                blocksRef.current.set(messageId, nextBlocks);
                                return message;
                            }

                            if (!nextBlocks.some((block) => block.type === 'tool_call' && block.id === toolCallId)) {
                                nextBlocks.push({ type: 'tool_call', id: toolCallId });
                            }
                            blocksRef.current.set(messageId, nextBlocks);

                            return {
                                ...message,
                                content_before_tools: message.content_before_tools !== undefined
                                    ? message.content_before_tools
                                    : (accumulatedContentRef.current.id === messageId
                                        ? accumulatedContentRef.current.content
                                        : message.content),
                                tool_calls: [
                                    ...existingTools,
                                    {
                                        ...(incomingToolCall as ToolCall),
                                        status,
                                        ...(result ? { result } : {}),
                                    },
                                ],
                                blocks: nextBlocks,
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
            unlistenCommand?.();
            unlistenToolCompleted?.();
            unlistenTodoUpdated?.();
            unlistenBladeEvent?.();
            if (flushScheduledRef.current !== null) {
                window.clearTimeout(flushScheduledRef.current);
                flushScheduledRef.current = null;
            }
            clearPendingTimers();
        };
    }, [clearPendingTimers, flushPendingUpdates, queueMessageUpdate, setMessages, setToolActivity, updateMessages, updateToolCallsStatusLocally]);

    const dispatchToBackend = useCallback(async (text: string, attachments?: ImageAttachment[], mentions?: ComposerMention[], mode?: ChatMode) => {
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
        } catch (error) {
            console.error('[useChatV2] Failed to send message:', error);
            dispatchInFlightRef.current = false;
            dispatch({ type: 'error/set', error: error instanceof Error ? error.message : String(error) });
            dispatch({ type: 'loading/set', loading: false });
        }
    }, [requestFreshEditorContext, toChatMentions]);

    useEffect(() => {
        if (state.loading || dispatchInFlightRef.current || state.messageQueue.length === 0) {
            return;
        }
        dispatchInFlightRef.current = true;
        const nextMessage = state.messageQueue[0];
        dispatch({ type: 'queue/shift' });
        void dispatchToBackend(nextMessage.text, nextMessage.attachments, nextMessage.mentions, nextMessage.mode);
    }, [dispatchToBackend, state.loading, state.messageQueue]);

    const sendMessage = useCallback((text: string, attachments?: ImageAttachment[], mentions?: ComposerMention[], mode?: ChatMode) => {
        const requestMode = mode ?? chatModeRef.current;
        const userMessage: ChatMessage = {
            id: crypto.randomUUID(),
            role: 'User',
            content: text,
            images: attachments,
            mentions,
        };
        updateMessages((messages) => [...messages, userMessage]);
        dispatch({ type: 'queue/enqueue', request: { text, attachments, mentions, mode: requestMode } });
    }, [updateMessages]);

    const setChatMode = useCallback((mode: ChatMode) => {
        dispatch({ type: 'mode/set', mode });
    }, []);

    const deleteQueuedRequest = useCallback((index: number) => {
        dispatch({ type: 'queue/delete', index });
    }, []);

    const stopGeneration = useCallback(async () => {
        const inFlightToolCallIds = Array.from(new Set([
            ...messagesRef.current.flatMap((message) => (message.tool_calls || [])
                .filter((toolCall) => toolCall.status === undefined || toolCall.status === 'pending' || toolCall.status === 'executing')
                .map((toolCall) => toolCall.id)),
            ...(pendingActionsRef.current?.map((action) => action.id) || []),
        ]));

        if (inFlightToolCallIds.length > 0) {
            markToolCallsSkippedLocally(inFlightToolCallIds, 'Stopped by user');
        }

        toolChunkCountsRef.current.clear();
        setToolActivity(null);
        dispatch({ type: 'pending-actions/set', actions: null });

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
            await BladeDispatcher.workflow({
                type: 'ApproveToolDecision',
                payload: { decision },
            });
        } catch (error) {
            console.error('[useChatV2] Failed to approve tool decision:', error);
        }
    }, [markToolCallsSkippedLocally]);

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

    const newConversation = useCallback(async () => {
        try {
            await BladeDispatcher.chat({
                type: 'NewConversation',
                payload: { model: selectedModelIdRef.current },
            });
            resetStreamingState();
            firstDispatchRef.current = true;
            dispatch({ type: 'messages/replace', messages: [] });
            dispatch({ type: 'todos/set', todos: [] });
            dispatch({ type: 'queue/clear' });
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
        dispatch({ type: 'todos/set', todos: [] });
        dispatch({ type: 'queue/clear' });
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

    return {
        messages: state.messages,
        loading: state.loading,
        error: state.error,
        sendMessage,
        stopGeneration,
        models: state.models,
        refreshModels,
        selectedModelId: state.selectedModelId,
        setSelectedModelId,
        chatMode: state.chatMode,
        setChatMode,
        pendingActions: state.pendingActions,
        approveTool,
        approveToolDecision,
        skipSingleCommand,
        newConversation,
        undoTool,
        setConversation,
        loadConversation,
        toolActivity: state.toolActivity,
        activeTodos: state.activeTodos,
        setActiveTodos,
        messageQueue: state.messageQueue,
        deleteQueuedRequest,
    };
}
