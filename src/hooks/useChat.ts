import { useState, useEffect, useCallback, useRef } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import { EditorFacade } from '../services/editorFacade';
import type { ChatMessage, ImageAttachment, ModelInfo, QueuedRequest, ToolActivityState, ToolCall, StreamingState } from '../types/chat';
import type { Change } from '../types/change';
import { EventNames, type RequestConfirmationPayload, type StructuredAction, type ChangeAppliedPayload, type AllEditsAppliedPayload, type ToolExecutionCompletedPayload } from '../types/events';
import { useEditorState } from '../contexts/EditorContext';
import { MessageBuffer } from '../utils/eventBuffer';
import type { BladeEventEnvelope } from '../types/blade';
import { getOrCreateIdempotencyKey, IDEMPOTENT_OPERATIONS } from '../utils/idempotency';
import { ensureMessagesHaveBlocks } from '../utils/messageBlocks';

const MAX_DELTA_OVERLAP_CHECK = 512;
const MIN_SNAPSHOT_PREFIX = 48;

function commonPrefixLength(a: string, b: string): number {
    const max = Math.min(a.length, b.length);
    let i = 0;
    while (i < max && a.charCodeAt(i) === b.charCodeAt(i)) i++;
    return i;
}

type StreamMergeResult = {
    next: string;
    append: string;
    changed: boolean;
    replaced: boolean;
};

function mergeStreamChunk(previous: string, incoming: string): StreamMergeResult {
    if (!incoming) {
        return { next: previous, append: '', changed: false, replaced: false };
    }

    if (!previous) {
        return { next: incoming, append: incoming, changed: true, replaced: false };
    }

    // Exact duplicate of the just-emitted suffix; drop it.
    if (previous.endsWith(incoming)) {
        return { next: previous, append: '', changed: false, replaced: false };
    }

    // Some providers emit cumulative snapshots instead of strict deltas.
    // Keep only the newly extended suffix.
    if (incoming.startsWith(previous)) {
        const delta = incoming.slice(previous.length);
        return { next: incoming, append: delta, changed: delta.length > 0, replaced: false };
    }

    // Late/stale replay of an earlier contiguous segment.
    if (previous.includes(incoming)) {
        return { next: previous, append: '', changed: false, replaced: false };
    }

    // Some providers occasionally emit overlapping snapshots/chunks.
    // Keep only the non-overlapping suffix so text doesn't become garbled.
    const prevTail = previous.slice(-MAX_DELTA_OVERLAP_CHECK);
    const incomingHead = incoming.slice(0, MAX_DELTA_OVERLAP_CHECK);
    const maxOverlap = Math.min(prevTail.length, incomingHead.length);

    for (let overlap = maxOverlap; overlap > 0; overlap--) {
        if (prevTail.slice(-overlap) === incomingHead.slice(0, overlap)) {
            const delta = incoming.slice(overlap);
            return {
                next: previous + delta,
                append: delta,
                changed: delta.length > 0,
                replaced: false,
            };
        }
    }

    // Snapshot-with-revision case: provider resent a near-complete authoritative snapshot
    // and revised text earlier in the response. Replace instead of append to avoid gibberish.
    const prefixLen = commonPrefixLength(previous, incoming);
    if (incoming.length >= Math.max(64, Math.floor(previous.length * 0.7)) && prefixLen >= 12) {
        return {
            next: incoming,
            append: '',
            changed: incoming !== previous,
            replaced: true,
        };
    }

    // We have a strong shared prefix but this is not a full snapshot replacement.
    // Keep only rewritten tail to avoid duplicate prefixes.
    if (prefixLen >= MIN_SNAPSHOT_PREFIX) {
        const rewrittenTail = incoming.slice(prefixLen);
        return {
            next: previous + rewrittenTail,
            append: rewrittenTail,
            changed: rewrittenTail.length > 0,
            replaced: false,
        };
    }

    return {
        next: previous + incoming,
        append: incoming,
        changed: true,
        replaced: false,
    };
}

export function useChat() {
    const editorState = useEditorState();
    const editorStateRef = useRef(editorState);
    const firstDispatchRef = useRef(true);
    const [messages, setMessages] = useState<ChatMessage[]>([]);
    const messagesRef = useRef<ChatMessage[]>([]);
    const blocksRef = useRef<Map<string, import('../types/chat').MessageBlock[]>>(new Map());
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    
    // Tool activity state for per-tool-call streaming progress display
    const [toolActivity, setToolActivity] = useState<ToolActivityState | null>(null);
    const toolChunkCountsRef = useRef<Map<string, { chunkCount: number; startedAt: number; lastChunkAt: number }>>(new Map());
    const pendingTimeoutsRef = useRef<number[]>([]);

    // Active todo list state — lifted out of messages for persistent TaskPanel
    const [activeTodos, setActiveTodos] = useState<import('../types/events').TodoItem[]>([]);

    useEffect(() => {
        editorStateRef.current = editorState;
    }, [editorState]);

    const buildEditorContext = useCallback((state: {
        activeFile: string | null;
        openFiles: string[];
        cursorLine: number | null;
        cursorColumn: number | null;
        selectionStartLine: number | null;
        selectionEndLine: number | null;
    }) => {
        const safeActiveFile = state.activeFile || null;
        const openFromState = state.openFiles.length > 0
            ? state.openFiles
            : (safeActiveFile ? [safeActiveFile] : []);
        const normalizedOpenFiles = Array.from(new Set(openFromState.filter(Boolean)));

        return {
            active_file: safeActiveFile,
            open_files: normalizedOpenFiles,
            cursor_line: state.cursorLine ?? null,
            cursor_column: state.cursorColumn ?? null,
            selection_start: state.selectionStartLine ?? null,
            selection_end: state.selectionEndLine ?? null,
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
                    if (bladeEvent.type !== 'Editor') return;
                    const payload = bladeEvent.payload as import('../types/blade').EditorEvent;
                    if (payload.type !== 'StateSnapshot') return;

                    window.clearTimeout(timeout);
                    if (unlisten) {
                        unlisten();
                        unlisten = undefined;
                    }
                    resolve(payload.payload);
                })
                    .then((fn) => {
                        unlisten = fn;
                    })
                    .catch((err) => {
                        window.clearTimeout(timeout);
                        reject(err);
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

    // Reset all streaming infrastructure — must be called when switching conversations
    const resetStreamingState = useCallback(() => {
        if (messageBufferRef.current) {
            messageBufferRef.current.clearAll();
        }
        accumulatedContentRef.current = { id: '', content: '' };
        accumulatedReasoningRef.current = { id: '', content: '' };
        blocksRef.current.clear();
        pendingUpdatesRef.current.clear();
        streamingStatesRef.current.clear();
        if (flushScheduledRef.current) {
            clearTimeout(flushScheduledRef.current);
            flushScheduledRef.current = null;
        }
        toolChunkCountsRef.current.clear();
        if (pendingTimeoutsRef.current.length > 0) {
            pendingTimeoutsRef.current.forEach(id => clearTimeout(id));
            pendingTimeoutsRef.current = [];
        }
        setToolActivity(null);
        setLoading(false);
        setPendingActions(null);
    }, []);

    // v1.1: Message buffer and accumulation ref for atomic updates
    const messageBufferRef = useRef<MessageBuffer | null>(null);
    const accumulatedContentRef = useRef<{ id: string; content: string }>({ id: '', content: '' });
    const accumulatedReasoningRef = useRef<{ id: string; content: string }>({ id: '', content: '' });

    // v1.2: Batched rendering - buffer updates and flush at intervals
    // This prevents re-rendering on every single streaming chunk
    const pendingUpdatesRef = useRef<Map<string, { content: string; reasoning: string; blocks: import('../types/chat').MessageBlock[]; streaming?: StreamingState }>>(new Map());
    const flushScheduledRef = useRef<number | null>(null);
    const FLUSH_INTERVAL_MS = 80; // ~12.5fps - reduces render pressure while remaining responsive
    const streamingStatesRef = useRef<Map<string, StreamingState>>(new Map());

    // Flush pending updates to state
    const flushPendingUpdates = useCallback(() => {
        flushScheduledRef.current = null;
        const pending = pendingUpdatesRef.current;
        if (pending.size === 0) return;

        setMessages(prev => {
            let updated = prev;
            let changed = false;

            // Build id→index lookup once per flush instead of findIndex per pending update
            const idxById = new Map<string, number>();
            for (let i = 0; i < prev.length; i++) {
                const mid = prev[i].id;
                if (mid) idxById.set(mid, i);
            }

            pending.forEach((update, id) => {
                const idx = idxById.get(id) ?? -1;
                if (idx !== -1) {
                    // Update existing message
                    const msg = updated[idx];
                    const streamingChanged =
                        (msg.streaming?.seq ?? null) !== (update.streaming?.seq ?? null)
                        || (msg.streaming?.endTime ?? null) !== (update.streaming?.endTime ?? null);

                    if (msg.content !== update.content || msg.reasoning !== update.reasoning || streamingChanged) {
                        if (!changed) {
                            updated = [...prev];
                            changed = true;
                        }
                        // Use blocks from blocksRef (via update.blocks) directly — they already
                        // maintain the correct interleaved order (text → tool_call → text, etc.).
                        // Only inject non-text blocks from msg.blocks that blocksRef doesn't know about
                        // (e.g. tool_call blocks added by ToolUpdate between flush cycles).
                        const updateBlockIds = new Set(update.blocks.map(b => b.id));
                        const missingNonTextBlocks = (msg.blocks || []).filter(
                            b => b.type !== 'text' && b.type !== 'reasoning' && !updateBlockIds.has(b.id)
                        );
                        // Append any missing non-text blocks at their natural position (end of existing non-text run)
                        let mergedBlocks = [...update.blocks];
                        if (missingNonTextBlocks.length > 0) {
                            // Find the last non-text block in update.blocks to insert after
                            let insertIdx = mergedBlocks.length;
                            for (let i = mergedBlocks.length - 1; i >= 0; i--) {
                                if (mergedBlocks[i].type !== 'text' && mergedBlocks[i].type !== 'reasoning') {
                                    insertIdx = i + 1;
                                    break;
                                }
                            }
                            mergedBlocks.splice(insertIdx, 0, ...missingNonTextBlocks);
                        }
                        
                        updated[idx] = {
                            ...msg,
                            content: update.content,
                            reasoning: update.reasoning,
                            blocks: mergedBlocks,
                            streaming: update.streaming,
                        };
                    }
                } else {
                    // Create new message.
                    // IMPORTANT: append in chronological order.
                    // Inserting after "last user" can invert assistant message order when
                    // multiple new assistant IDs land in the same batched flush.
                    if (!changed) {
                        updated = [...prev];
                        changed = true;
                    }
                    const newMsg = {
                        id,
                        role: 'Assistant',
                        content: update.content,
                        reasoning: update.reasoning,
                        blocks: update.blocks,
                        streaming: update.streaming,
                    } as ChatMessage;
                    
                    updated.push(newMsg);
                }
            });

            pending.clear();
            return changed ? updated : prev;
        });
    }, []);

    // Schedule a flush if not already scheduled
    const scheduleFlush = useCallback(() => {
        if (flushScheduledRef.current === null) {
            flushScheduledRef.current = window.setTimeout(flushPendingUpdates, FLUSH_INTERVAL_MS);
        }
    }, [flushPendingUpdates]);

    // Queue an update for batched rendering
    const queueMessageUpdate = useCallback((
        id: string,
        content: string,
        reasoning: string,
        blocks: import('../types/chat').MessageBlock[],
        streaming?: StreamingState,
    ) => {
        pendingUpdatesRef.current.set(id, { content, reasoning, blocks, streaming });
        scheduleFlush();
    }, [scheduleFlush]);

    const [models, setModels] = useState<ModelInfo[]>([]);
    const [selectedModelId, setSelectedModelIdState] = useState<string>('anthropic/claude-sonnet-4-5-20250929');
    const selectedModelIdRef = useRef<string>('anthropic/claude-sonnet-4-5-20250929');
    const hasExplicitModelRef = useRef(false);

    const refreshModels = useCallback(async () => {
        try {
            const modelList = await invoke<ModelInfo[]>('list_models');
            setModels(modelList);
            return modelList;
        } catch (e) {
            console.error('[useChat] Failed to refresh models:', e);
            throw e;
        }
    }, []);

    const updateToolCallsStatusLocally = useCallback((
        toolCallIds: string[],
        nextStatus: 'pending' | 'executing' | 'complete' | 'error' | 'skipped',
        fallbackResultText: string,
    ) => {
        if (toolCallIds.length === 0) return;
        const idSet = new Set(toolCallIds);
        setMessages(prev => prev.map(msg => {
            if (!msg.tool_calls?.some(tc => idSet.has(tc.id))) return msg;
            return {
                ...msg,
                tool_calls: (msg.tool_calls || []).map(tc =>
                    idSet.has(tc.id)
                        ? {
                            ...tc,
                            status: nextStatus,
                            ...(tc.result ? {} : { result: fallbackResultText }),
                        }
                        : tc,
                ),
            };
        }));
    }, []);

    const markToolCallsSkippedLocally = useCallback((
        toolCallIds: string[],
        skippedResultText = 'Skipped by user',
    ) => {
        updateToolCallsStatusLocally(toolCallIds, 'skipped', skippedResultText);
    }, [updateToolCallsStatusLocally]);

    // Wrapper that syncs with backend when model changes
    const setSelectedModelId = useCallback(async (modelId: string) => {
        hasExplicitModelRef.current = true;
        selectedModelIdRef.current = modelId;
        setSelectedModelIdState(modelId);
        try {
            await BladeDispatcher.chat({
                type: 'SetSelectedModel',
                payload: { model: modelId }
            });
            console.debug('[useChat] Synced model to backend:', modelId);
        } catch (e) {
            console.error('[useChat] Failed to sync model to backend:', e);
        }
    }, []);

    const logFrontend = useCallback(async (message: string) => {
        try {
            await invoke('log_frontend', { message });
        } catch (e) {
            console.error('[useChat] log_frontend failed', e);
        }
    }, []);

    // Permission Logic
    const [pendingActions, setPendingActions] = useState<StructuredAction[] | null>(null);

    // Load initial conversation and models
    useEffect(() => {
        async function init() {
            try {
                // Ensure we are in a window context (client-side) and have Tauri
                if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
                    console.debug('Not in Tauri environment, skipping chat init');
                    return;
                }

                const [history, modelList, isStreaming] = await Promise.all([
                    invoke<ChatMessage[]>('get_conversation'),
                    invoke<ModelInfo[]>('list_models'),
                    invoke<boolean>('get_chat_status'),
                ]);

                console.debug('Loaded conversation:', history);
                // Reconstruct blocks for historical messages
                setMessages(ensureMessagesHaveBlocks(history));
                setModels(modelList);

                // Restore loading state if backend is still streaming (e.g. after UI reload)
                if (isStreaming) {
                    console.debug('[useChat] Backend is still streaming — restoring loading state');
                    setLoading(true);
                }

                // Set a default model - project state will override this if available
                // This prevents the model from being undefined before project state loads
                if (modelList.length > 0 && !hasExplicitModelRef.current) {
                    const defaultModel = modelList.find(m => m.id === 'anthropic/claude-sonnet-4-5-20250929')
                        || modelList.find(m => m.id === 'openai/gpt-5.2')
                        || modelList[0];
                    setSelectedModelIdState(defaultModel.id);
                    console.debug('[useChat] Set initial default model:', defaultModel.id);
                }

            } catch (e) {
                console.error('Failed to init:', e);
                // Don't show error if it's just because backend isn't ready or we are server-side
            }
        }
        init();
    }, []);

    useEffect(() => {
        selectedModelIdRef.current = selectedModelId;
    }, [selectedModelId]);

    useEffect(() => {
        messagesRef.current = messages;
    }, [messages]);

    // Listen for updates
    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;

        let unlistenUpdate: (() => void) | undefined;
        let unlistenDone: (() => void) | undefined;
        let unlistenError: (() => void) | undefined;
        let unlistenContextLength: (() => void) | undefined;
        let unlistenPerm: (() => void) | undefined;
        let unlistenChanges: (() => void) | undefined;
        let unlistenCommand: (() => void) | undefined;
        let unlistenToolCompleted: (() => void) | undefined;
        let unlistenV11: (() => void) | undefined;

        // Initialize v1.1 message buffer
        // v1.2: Use batched rendering - accumulate in refs, queue updates at intervals
        if (!messageBufferRef.current) {
            messageBufferRef.current = new MessageBuffer(
                (id, seq, chunk, is_final, type) => {
                    // NOTE: Do NOT call setLoading(true) here. Loading is set in dispatchToBackend.
                    // Calling it on every chunk creates a race condition: if a chunk flushes after
                    // chat-done/MessageCompleted sets loading=false, it re-sets loading=true permanently.

                    const now = Date.now();
                    const prevStreaming = streamingStatesRef.current.get(id);
                    const streaming: StreamingState = {
                        seq: Math.max(seq, prevStreaming?.seq ?? 0),
                        startTime: prevStreaming?.startTime ?? now,
                        lastSeqAt: now,
                    };
                    streamingStatesRef.current.set(id, streaming);

                    // Accumulate content/reasoning in refs (no re-render)
                    // When ID changes, this indicates a new message stream - clear stale blocks
                    let mergedChunk: StreamMergeResult;
                    if (type === 'reasoning') {
                        if (accumulatedReasoningRef.current.id !== id) {
                            accumulatedReasoningRef.current = { id, content: '' };
                            // New reasoning stream - clear stale reasoning blocks from blocksRef
                            const existingBlocks = blocksRef.current.get(id) || [];
                            if (existingBlocks.length > 0) {
                                const nonReasoningBlocks = existingBlocks.filter(b => b.type !== 'reasoning');
                                blocksRef.current.set(id, nonReasoningBlocks);
                            }
                        }
                        mergedChunk = mergeStreamChunk(accumulatedReasoningRef.current.content, chunk);
                        if (!mergedChunk.changed) {
                            queueMessageUpdate(
                                id,
                                accumulatedContentRef.current.id === id ? accumulatedContentRef.current.content : '',
                                accumulatedReasoningRef.current.id === id ? accumulatedReasoningRef.current.content : '',
                                blocksRef.current.get(id) || [],
                                streaming,
                            );
                            return;
                        }
                        accumulatedReasoningRef.current.content = mergedChunk.next;
                    } else {
                        if (accumulatedContentRef.current.id !== id) {
                            accumulatedContentRef.current = { id, content: '' };
                            // New content stream for this message - clear stale text blocks from blocksRef.
                            // IMPORTANT: keep reasoning blocks so chain-of-thought summary remains visible
                            // when the assistant transitions from reasoning to final answer.
                            const existingBlocks = blocksRef.current.get(id) || [];
                            if (existingBlocks.length > 0) {
                                const nonTextBlocks = existingBlocks.filter(b => b.type !== 'text');
                                blocksRef.current.set(id, nonTextBlocks);
                            }
                        }
                        mergedChunk = mergeStreamChunk(accumulatedContentRef.current.content, chunk);
                        if (!mergedChunk.changed) {
                            queueMessageUpdate(
                                id,
                                accumulatedContentRef.current.id === id ? accumulatedContentRef.current.content : '',
                                accumulatedReasoningRef.current.id === id ? accumulatedReasoningRef.current.content : '',
                                blocksRef.current.get(id) || [],
                                streaming,
                            );
                            return;
                        }
                        accumulatedContentRef.current.content = mergedChunk.next;
                    }

                    // Build blocks structure using existing message order (includes tool_call blocks)
                    // CRITICAL: Prioritize blocksRef (synchronous) over existingMsg.blocks (async/stale)
                    // to prevent race conditions where stale data overwrites fresh accumulated blocks
                    const existingMsg = messagesRef.current.find(m => m.id === id);
                    let blocks = blocksRef.current.get(id) || [];
                    
                    // Only use existingMsg.blocks if blocksRef is empty AND existingMsg has non-text blocks
                    // This preserves tool_call blocks from the message state while keeping fresh text blocks
                    if (blocks.length === 0 && existingMsg?.blocks && existingMsg.blocks.length > 0) {
                        // Only copy non-text/non-reasoning blocks (tool_call, command_execution, etc.)
                        // Text/reasoning blocks should come from the fresh stream, not stale state
                        blocks = existingMsg.blocks.filter(b => b.type !== 'text' && b.type !== 'reasoning');
                    }

                    const lastBlock = blocks[blocks.length - 1];
                    
                    if (type === 'reasoning') {
                        const fullReasoning = accumulatedReasoningRef.current.id === id
                            ? accumulatedReasoningRef.current.content
                            : mergedChunk.next;
                        // Create new reasoning block if:
                        // 1. No blocks exist yet
                        // 2. Last block is not reasoning (text or tool_call)
                        // This ensures reasoning after tool calls gets its own block
                        if (mergedChunk.replaced) {
                            const lastReasoningIdx = (() => {
                                for (let i = blocks.length - 1; i >= 0; i--) {
                                    if (blocks[i].type === 'reasoning') return i;
                                }
                                return -1;
                            })();
                            if (lastReasoningIdx >= 0) {
                                const targetBlock = blocks[lastReasoningIdx];
                                if (targetBlock && targetBlock.type === 'reasoning') {
                                    blocks[lastReasoningIdx] = { ...targetBlock, content: fullReasoning };
                                }
                            } else {
                                blocks = [...blocks, { type: 'reasoning', content: fullReasoning, id: crypto.randomUUID() }];
                            }
                        } else if (lastBlock && lastBlock.type === 'reasoning') {
                            // Append to existing reasoning block (continuous reasoning)
                            blocks[blocks.length - 1] = { ...lastBlock, content: lastBlock.content + mergedChunk.append };
                        } else {
                            // Create new reasoning block (after text, tool_call, or first block)
                            blocks = [...blocks, { type: 'reasoning', content: mergedChunk.append, id: crypto.randomUUID() }];
                        }
                    } else {
                        const fullContent = accumulatedContentRef.current.id === id
                            ? accumulatedContentRef.current.content
                            : mergedChunk.next;
                        if (mergedChunk.replaced) {
                            const lastTextIdx = (() => {
                                for (let i = blocks.length - 1; i >= 0; i--) {
                                    if (blocks[i].type === 'text') return i;
                                }
                                return -1;
                            })();
                            if (lastTextIdx >= 0) {
                                const targetBlock = blocks[lastTextIdx];
                                if (targetBlock && targetBlock.type === 'text') {
                                    blocks[lastTextIdx] = { ...targetBlock, content: fullContent };
                                }
                            } else {
                                blocks = [...blocks, { type: 'text', content: fullContent, id: crypto.randomUUID() }];
                            }
                        } else if (lastBlock && lastBlock.type === 'text') {
                            // Append to existing text block
                            blocks[blocks.length - 1] = { ...lastBlock, content: lastBlock.content + mergedChunk.append };
                        } else {
                            // Create new text block
                            blocks = [...blocks, { type: 'text', content: mergedChunk.append, id: crypto.randomUUID() }];
                        }
                    }
                    blocksRef.current.set(id, blocks);

                    // Queue batched update (will flush at 50ms intervals)
                    queueMessageUpdate(
                        id,
                        accumulatedContentRef.current.id === id ? accumulatedContentRef.current.content : '',
                        accumulatedReasoningRef.current.id === id ? accumulatedReasoningRef.current.content : '',
                        blocks,
                        streaming,
                    );
                },
                (id) => {
                    const prevStreaming = streamingStatesRef.current.get(id);
                    const streaming = prevStreaming
                        ? { ...prevStreaming, endTime: Date.now() }
                        : undefined;
                    if (streaming) {
                        streamingStatesRef.current.set(id, streaming);
                    }

                    const existingMsg = messagesRef.current.find(m => m.id === id);
                    const blocks = blocksRef.current.get(id) || existingMsg?.blocks || [];

                    queueMessageUpdate(
                        id,
                        accumulatedContentRef.current.id === id
                            ? accumulatedContentRef.current.content
                            : (existingMsg?.content || ''),
                        accumulatedReasoningRef.current.id === id
                            ? accumulatedReasoningRef.current.content
                            : (existingMsg?.reasoning || ''),
                        blocks,
                        streaming,
                    );
                    // Message completed - flush immediately and cleanup
                    flushPendingUpdates();
                    blocksRef.current.delete(id);
                    setLoading(false);
                }
            );
        }

        const setupListeners = async () => {
            // v1.1 MIGRATION: Legacy chat-update listener removed.
            // We now rely entirely on blade-event for text (MessageDelta) and tool status (ToolUpdate).
            /*
            const u1 = await listen<ChatMessage>('chat-update', (event) => {
                const msg = event.payload;
                console.debug('[CHAT UPDATE]', msg);
                // ... legacy logic ...
                 setMessages((prev) => {
                     // ... 
                     return prev;
                 });
            });
            unlistenUpdate = u1;
            */

            const u2 = await listen('chat-done', () => {
                const inFlightToolCallIds = Array.from(new Set(
                    messagesRef.current.flatMap(msg =>
                        (msg.tool_calls || [])
                            .filter(tc => tc.status === undefined || tc.status === 'pending' || tc.status === 'executing')
                            .map(tc => tc.id)
                    )
                ));
                if (inFlightToolCallIds.length > 0) {
                    updateToolCallsStatusLocally(
                        inFlightToolCallIds,
                        'complete',
                        'Completed after conversation ended',
                    );
                }

                setLoading(false);
                setPendingActions(null); // Clear any hanging dialogs

                // Auto-complete lingering todos when chat finishes.
                // Models sometimes forget to send a final todo_write marking the last task as completed.
                // Wait briefly to allow any in-flight todo_updated events to arrive first.
                const settleTodosTimer = window.setTimeout(() => {
                    setActiveTodos(prev => {
                        if (prev.length === 0) return prev;
                        const hasIncomplete = prev.some(t => t.status !== 'completed');
                        if (!hasIncomplete) return prev; // Already all completed, normal flow handles it
                        // Mark all remaining items as completed
                        const completed = prev.map(t => ({ ...t, status: 'completed' as const }));
                        // Trigger the completion flow (clear panel + insert summary) after brief display
                        const finalizeTodosTimer = window.setTimeout(() => {
                            setActiveTodos([]);
                            const summaryId = `plan-summary-${Date.now()}`;
                            const summaryMessage: ChatMessage = {
                                id: summaryId,
                                role: 'Assistant',
                                content: '',
                                blocks: [{ type: 'plan_summary' as const, id: summaryId }],
                                planSummary: {
                                    todos: [...completed],
                                    completedAt: Date.now(),
                                },
                            };
                            setMessages(prev => [...prev, summaryMessage]);
                        }, 1500);
                        pendingTimeoutsRef.current.push(finalizeTodosTimer);
                        return completed;
                    });
                }, 500);
                pendingTimeoutsRef.current.push(settleTodosTimer);
            });
            unlistenDone = u2;

            const u3 = await listen<string>('chat-error', (event) => {
                setLoading(false);
                setPendingActions(null);
                setError(event.payload);
            });
            unlistenError = u3;

            // RFC: Context Length Recovery - listen for context limit exceeded events
            const uContextLength = await listen<{
                message: string;
                token_count: number | null;
                max_tokens: number | null;
                excess: number | null;
                recoverable: boolean;
                recovery_hint: string | null;
            }>('context-length-exceeded', (event) => {
                console.debug('[useChat] Context length exceeded:', event.payload);
                const { message, token_count, max_tokens, recoverable, recovery_hint } = event.payload;
                
                setLoading(false);
                setPendingActions(null);
                
                // Show a user-friendly notification in the chat
                const tokenInfo = token_count && max_tokens 
                    ? ` (${token_count.toLocaleString()} / ${max_tokens.toLocaleString()} tokens)`
                    : '';
                
                // Add a system message to the chat to inform the user
                const msgId = `system-context-${Date.now()}`;
                const systemMessage: ChatMessage = {
                    id: msgId,
                    role: 'Assistant',
                    content: `⚠️ **Context Limit Reached**${tokenInfo}\n\n` +
                        `${message}\n\n` +
                        (recoverable 
                            ? (recovery_hint || 'The AI is attempting to recover automatically. You can also try:\n' +
                              '- Starting a new conversation\n' +
                              '- Asking the AI to summarize the conversation')
                            : 'Please start a new conversation to continue.'),
                    blocks: [{ type: 'text', content: '', id: msgId }],
                };
                setMessages(prev => [...prev, systemMessage]);
            });
            unlistenContextLength = uContextLength;

            // Listen for message-too-large errors
            const uMessageTooLarge = await listen<{
                message: string;
                recovery_hint: string;
            }>('message-too-large', (event) => {
                console.debug('[useChat] Message too large:', event.payload);
                const { message, recovery_hint } = event.payload;
                
                setLoading(false);
                
                // Add a system message to the chat to inform the user
                const msgId = `system-size-${Date.now()}`;
                const systemMessage: ChatMessage = {
                    id: msgId,
                    role: 'Assistant',
                    content: `⚠️ **Response Too Large**\n\n` +
                        `${message}\n\n` +
                        `**Recovery hint:** ${recovery_hint}`,
                    blocks: [{ type: 'text', content: '', id: msgId }],
                };
                setMessages(prev => [...prev, systemMessage]);
            });
            let unlistenMessageTooLarge = uMessageTooLarge;

            // Listen for permission requests
            const u4 = await listen<RequestConfirmationPayload>('request-confirmation', (event) => {
                console.debug("Permission requested for:", event.payload);
                setPendingActions(event.payload.actions);
            });
            unlistenPerm = u4;



            // Listen for command executions
            const u6 = await listen<{ command: string; cwd?: string; output: string; exitCode: number; duration?: number; call_id: string }>('command-executed', (event) => {
                const { command, cwd, output, exitCode, duration, call_id } = event.payload;
                console.debug('[COMMAND EXECUTED]', { command, call_id, exitCode });

                setMessages(prev => {
                    // 1. Find the message containing this tool call ID
                    const msgIndex = prev.findIndex(m =>
                        m.tool_calls?.some(tc => tc.id === call_id)
                    );

                    if (msgIndex === -1) {
                        console.warn('[COMMAND EXECUTED] Could not find message for call_id:', call_id);
                        return prev;
                    }

                    const updated = [...prev];
                    const msg = { ...updated[msgIndex] };

                    // 2. Add to commandExecutions array (use call_id as execution ID)
                    const newExecution = {
                        id: call_id,
                        command,
                        cwd,
                        output,
                        exitCode,
                        duration,
                        timestamp: Date.now(),
                    };

                    // Avoid duplicates if event is received twice
                    const existingExecIndex = (msg.commandExecutions || []).findIndex(c => c.id === call_id);
                    let newExecutions = [...(msg.commandExecutions || [])];
                    if (existingExecIndex >= 0) {
                        newExecutions[existingExecIndex] = newExecution;
                    } else {
                        newExecutions.push(newExecution);
                    }

                    // 3. Update blocks for proper interleaving
                    // CRITICAL: Use blocksRef as source of truth (like ToolUpdate does)
                    // to avoid desync where blocksRef has newer tool_call blocks that
                    // msg.blocks doesn't know about yet.
                    const liveBlocks = blocksRef.current.get(msg.id!);
                    const newBlocks = liveBlocks ? [...liveBlocks] : [...(msg.blocks || [])];

                    // Find if block already exists
                    const existingBlockIndex = newBlocks.findIndex(b => b.type === 'command_execution' && b.id === call_id);

                    if (existingBlockIndex === -1) {
                        // Find the corresponding tool_call block to insert after it
                        const toolCallBlockIndex = newBlocks.findIndex(b => b.type === 'tool_call' && b.id === call_id);

                        if (toolCallBlockIndex >= 0) {
                            // Insert immediately after the tool call
                            newBlocks.splice(toolCallBlockIndex + 1, 0, { type: 'command_execution', id: call_id });
                        } else {
                            // Fallback: push to end if tool_call block not found (shouldn't happen)
                            newBlocks.push({ type: 'command_execution', id: call_id });
                        }
                    }

                    // Sync back to blocksRef so future MessageDelta flushes preserve this block
                    blocksRef.current.set(msg.id!, newBlocks);

                    updated[msgIndex] = {
                        ...msg,
                        blocks: newBlocks,
                        commandExecutions: newExecutions,
                    };
                    return updated;
                });
            });
            unlistenCommand = u6;

            const u7 = await listen<ToolExecutionCompletedPayload>('tool-execution-completed', (event) => {
                const { tool_call_id, success, skipped } = event.payload;
                const nextStatus: 'complete' | 'error' | 'skipped' = skipped
                    ? 'skipped'
                    : success
                        ? 'complete'
                        : 'error';

                setMessages(prev => prev.map(msg => {
                    if (!msg.tool_calls?.some(tc => tc.id === tool_call_id)) return msg;
                    return {
                        ...msg,
                        tool_calls: (msg.tool_calls || []).map(tc =>
                            tc.id === tool_call_id
                                ? {
                                    ...tc,
                                    status: nextStatus,
                                    ...(skipped && !tc.result ? { result: 'Skipped by user' } : {}),
                                }
                                : tc,
                        ),
                    };
                }));

                if (skipped) {
                    setPendingActions(prev => {
                        if (!prev || prev.length === 0) return prev;
                        const filtered = prev.filter(action => action.id !== tool_call_id);
                        return filtered.length > 0 ? filtered : null;
                    });
                }
            });
            unlistenToolCompleted = u7;



            // Listen for todo list updates — update shared state for TaskPanel
            const u10 = await listen<{ todos: import('../types/events').TodoItem[] }>(EventNames.TODO_UPDATED, (event) => {
                const todos = event.payload.todos;
                invoke('log_frontend', { message: `[FRONTEND] TODO_UPDATED received: ${todos.length} items` });
                setActiveTodos(todos);

                // Completion detection: all tasks done → brief "all done" state → hide panel + insert summary
                const allCompleted = todos.length > 0 && todos.every(t => t.status === 'completed');
                if (allCompleted) {
                    const todosCompletedTimer = window.setTimeout(() => {
                        setActiveTodos([]);
                        // Insert compact plan summary message into chat
                        const summaryId = `plan-summary-${Date.now()}`;
                        const summaryMessage: ChatMessage = {
                            id: summaryId,
                            role: 'Assistant',
                            content: '',
                            blocks: [{ type: 'plan_summary' as const, id: summaryId }],
                            planSummary: {
                                todos: [...todos],
                                completedAt: Date.now(),
                            },
                        };
                        setMessages(prev => [...prev, summaryMessage]);
                    }, 1500);
                    pendingTimeoutsRef.current.push(todosCompletedTimer);
                }
            });
            const unlistenTodoUpdated = u10;

            // v1.1: blade-event listener for MessageDelta with sequence numbers
            const u11 = await listen<BladeEventEnvelope>('blade-event', (event) => {
                const envelope = event.payload;

                if (envelope.event.type === 'Chat') {
                    const chatEvent = envelope.event.payload;

                    if (chatEvent.type === 'MessageDelta') {
                        const { id, seq, chunk, is_final } = chatEvent.payload;

                        // Use buffer to handle out-of-order chunks
                        if (messageBufferRef.current) {
                            messageBufferRef.current.addMessageDelta(id, seq, chunk, is_final);
                        }
                    } else if (chatEvent.type === 'ReasoningDelta') {
                        const { id, seq, chunk, is_final } = chatEvent.payload;

                        if (messageBufferRef.current) {
                            messageBufferRef.current.addReasoningDelta(id, seq, chunk, is_final);
                        } else {
                            console.warn('[v1.1 Chat] ReasoningDelta received but messageBufferRef is null!');
                        }
                    } else if (chatEvent.type === 'MessageCompleted') {
                        const { id } = chatEvent.payload;

                        // Clear buffer for this message to prevent memory leaks or sequence issues
                        if (messageBufferRef.current) {
                            messageBufferRef.current.clear(id);
                        }
                        if (accumulatedContentRef.current.id === id) {
                            accumulatedContentRef.current = { id: '', content: '' };
                        }
                        if (accumulatedReasoningRef.current.id === id) {
                            accumulatedReasoningRef.current = { id: '', content: '' };
                        }

                        setLoading(false);
                        toolChunkCountsRef.current.clear();
                        setToolActivity(null);
                        // Buffer will auto-clear on is_final, but this provides explicit confirmation
                    } else if (chatEvent.type === 'ToolUpdate') {
                        const { message_id, tool_call_id, status, result, tool_call } = chatEvent.payload;

                        // Clear the tool activity preview — the real ToolCallDisplay takes over
                        toolChunkCountsRef.current.delete(tool_call_id);
                        setToolActivity(null);

                        setMessages(prev => {
                            const existingIdx = prev.findIndex(msg => msg.id === message_id);

                            if (existingIdx === -1) {
                                // Create new message for tool if missing.
                                // Keep any live streamed blocks (reasoning/text) so timeline order is preserved
                                // even when ToolUpdate arrives before a batched message flush.
                                const liveBlocks = blocksRef.current.get(message_id) || [];
                                const newBlocks = [...liveBlocks];
                                if (tool_call && !newBlocks.some(b => b.type === 'tool_call' && b.id === tool_call_id)) {
                                    newBlocks.push({ type: 'tool_call', id: tool_call_id });
                                }
                                blocksRef.current.set(message_id, newBlocks);

                                const newMsg: ChatMessage = {
                                    id: message_id,
                                    role: 'Assistant',
                                    content: '',
                                    tool_calls: tool_call ? [{ ...tool_call, status: status as any, result }] : [],
                                    blocks: newBlocks
                                };
                                // Insert after the last user message to maintain conversation flow
                                const lastUserIdx = prev.map(m => m.role).lastIndexOf('User');
                                if (lastUserIdx >= 0 && lastUserIdx === prev.length - 1) {
                                    // User message is at the end, append assistant after it
                                    return [...prev, newMsg];
                                }
                                return [...prev, newMsg];
                            }

                            return prev.map(msg => {
                                if (msg.id === message_id) {
                                    const existingTools = msg.tool_calls || [];
                                    const toolIndex = existingTools.findIndex(tc => tc.id === tool_call_id);
                                    let newTools = [...existingTools];
                                    // Preserve any in-flight text/reasoning blocks from blocksRef
                                    const liveBlocks = blocksRef.current.get(message_id);
                                    let newBlocks = liveBlocks ? [...liveBlocks] : [...(msg.blocks || [])];

                                    if (toolIndex >= 0) {
                                        // Update existing tool
                                        newTools[toolIndex] = { ...newTools[toolIndex], status: status as any };
                                        if (result) newTools[toolIndex].result = result;
                                        if (tool_call) newTools[toolIndex] = { ...newTools[toolIndex], ...tool_call };
                                    } else {
                                        // Add new tool call
                                        if (tool_call) {
                                            const contentBefore = msg.content_before_tools !== undefined
                                                ? msg.content_before_tools
                                                : (accumulatedContentRef.current.id === message_id
                                                    ? accumulatedContentRef.current.content
                                                    : msg.content);
                                            
                                            // Check if block already exists (idempotency safety)
                                            if (!newBlocks.some(b => b.type === 'tool_call' && b.id === tool_call_id)) {
                                                // Preserve natural stream order: ToolUpdate arrives in sequence,
                                                // so append the tool block at the end of current live blocks.
                                                // This keeps reasoning/tool/text chronology intact.
                                                newBlocks.push({ type: 'tool_call', id: tool_call_id });
                                            }

                                            blocksRef.current.set(message_id, newBlocks);
                                            return {
                                                ...msg,
                                                content_before_tools: contentBefore,
                                                tool_calls: [...existingTools, tool_call],
                                                blocks: newBlocks
                                            };
                                        } else {
                                            console.warn('[v1.1 Chat] Received ToolUpdate for unknown tool but no tool_call data provided:', tool_call_id);
                                        }
                                    }
                                    blocksRef.current.set(message_id, newBlocks);
                                    return { ...msg, tool_calls: newTools, blocks: newBlocks }; // Return updated msg
                                }
                                return msg;
                            });
                        });
                    } else if (chatEvent.type === 'ToolActivity') {
                        // Handle tool activity events with live per-tool-call chunk counting.
                        const { tool_name, file_path, action, tool_call_id } = chatEvent.payload;
                        const key = tool_call_id || `${tool_name}:${file_path}`;
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

                        const activity: ToolActivityState = {
                            toolName: tool_name,
                            filePath: file_path,
                            action,
                            toolCallId: tool_call_id,
                            chunkCount: tracked.chunkCount,
                            startedAt: tracked.startedAt,
                            lastChunkAt: tracked.lastChunkAt,
                        };
                        setToolActivity(activity);

                        if (action !== 'streaming') {
                            toolChunkCountsRef.current.delete(key);
                            const clearToolActivityTimer = window.setTimeout(() => {
                                setToolActivity(prev => {
                                    if (!prev) return prev;
                                    const prevKey = prev.toolCallId || `${prev.toolName}:${prev.filePath}`;
                                    return prevKey === key ? null : prev;
                                });
                            }, 2000);
                            pendingTimeoutsRef.current.push(clearToolActivityTimer);
                        }
                    }
                }
            });
            unlistenV11 = u11;

            return () => {
                if (unlistenUpdate) unlistenUpdate();
                if (unlistenDone) unlistenDone();
                if (unlistenError) unlistenError();
                if (unlistenContextLength) unlistenContextLength();
                if (unlistenMessageTooLarge) unlistenMessageTooLarge();
                if (unlistenPerm) unlistenPerm();
                if (unlistenCommand) unlistenCommand();
                if (unlistenToolCompleted) unlistenToolCompleted();
                if (unlistenTodoUpdated) unlistenTodoUpdated();
                if (unlistenV11) unlistenV11();
            };
        };

        const cleanupPromise = setupListeners();

        return () => {
            cleanupPromise.then(cleanup => cleanup());
            // Cleanup any pending flush on unmount
            if (flushScheduledRef.current) {
                clearTimeout(flushScheduledRef.current);
            }
            if (pendingTimeoutsRef.current.length > 0) {
                pendingTimeoutsRef.current.forEach(id => clearTimeout(id));
                pendingTimeoutsRef.current = [];
            }
        };
    }, [queueMessageUpdate, flushPendingUpdates, updateToolCallsStatusLocally]);

    const [messageQueue, setMessageQueue] = useState<QueuedRequest[]>([]);

    const dispatchToBackend = useCallback(async (text: string, attachments?: ImageAttachment[]) => {
        try {
            setLoading(true);
            setError(null);

            const context = await requestFreshEditorContext();

            // Dispatch via Blade Protocol
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
                    context
                }
            });

            firstDispatchRef.current = false;

        } catch (e) {
            console.error('Failed to send message:', e);
            setError(e instanceof Error ? e.message : String(e));
            setLoading(false); // Ensure loading is cleared on immediate error
        }
    }, [
        requestFreshEditorContext,
    ]);

    // Queue processing effect
    useEffect(() => {
        if (!loading && messageQueue.length > 0) {
            const nextMessage = messageQueue[0];
            setMessageQueue(prev => prev.slice(1));
            dispatchToBackend(nextMessage.text, nextMessage.attachments);
        }
    }, [loading, messageQueue, dispatchToBackend]);

    const sendMessage = useCallback((text: string, attachments?: ImageAttachment[]) => {
        // Optimistically add user message
        const userMsg: ChatMessage = {
            id: crypto.randomUUID(),
            role: 'User',
            content: text,
            images: attachments
        };
        setMessages(prev => [...prev, userMsg]);

        // Add to queue for processing
        setMessageQueue(prev => [...prev, { text, attachments }]);
    }, [loading]);

    const deleteQueuedRequest = useCallback((index: number) => {
        setMessageQueue(prev => prev.filter((_, idx) => idx !== index));
    }, []);
    const stopGeneration = useCallback(async () => {
        const inFlightToolCallIds = Array.from(new Set([
            ...messagesRef.current.flatMap(msg =>
                (msg.tool_calls || [])
                    .filter(tc => tc.status === undefined || tc.status === 'pending' || tc.status === 'executing')
                    .map(tc => tc.id)
            ),
            ...(pendingActions?.map(action => action.id) || []),
        ]));

        if (inFlightToolCallIds.length > 0) {
            markToolCallsSkippedLocally(inFlightToolCallIds, 'Stopped by user');
        }

        toolChunkCountsRef.current.clear();
        setToolActivity(null);
        // Clear any pending command approvals when stopping
        setPendingActions(null);

        try {
            await BladeDispatcher.chat({ type: 'StopGeneration', payload: {} });
            setLoading(false);
        } catch (e) {
            console.error("Failed to stop generation:", e);
        }
    }, [pendingActions, markToolCallsSkippedLocally]);



    const approveTool = useCallback(async (approved: boolean) => {
        try {
            await BladeDispatcher.workflow({
                type: 'ApproveTool',
                payload: { approved }
            });
            // Don't clear pendingActions here - same race condition as approveToolDecision
        } catch (e) {
            console.error('Failed to approve tool:', e);
        }
    }, []);

    const approveToolDecision = useCallback(async (decision: string) => {
        try {
            const pendingIds = pendingActions?.map(action => action.id) || [];
            if (decision === 'reject' && pendingIds.length > 0) {
                // Optimistic UI: immediately flip tool cards to skipped.
                markToolCallsSkippedLocally(pendingIds);
            }

            // Optimistically clear pending actions for immediate UI feedback
            // New request-confirmation events will set new actions if needed
            setPendingActions(null);
            
            await BladeDispatcher.workflow({
                type: 'ApproveToolDecision',
                payload: { decision }
            });
        } catch (e) {
            console.error('Failed to approve tool decision:', e);
        }
    }, [pendingActions, markToolCallsSkippedLocally]);

    const skipSingleCommand = useCallback(async (callId: string) => {
        // Optimistic UI: immediately mark this tool call as skipped.
        markToolCallsSkippedLocally([callId]);
        setPendingActions(prev => {
            if (!prev || prev.length === 0) return prev;
            const filtered = prev.filter(action => action.id !== callId);
            return filtered.length > 0 ? filtered : null;
        });

        try {
            await invoke('approve_single_command', { callId, approved: false });
        } catch (e) {
            console.error('Failed to skip single command:', e);
        }
    }, [markToolCallsSkippedLocally]);

    const newConversation = useCallback(async () => {
        try {
            await BladeDispatcher.chat({
                type: 'NewConversation',
                payload: { model: selectedModelIdRef.current }
            });
            resetStreamingState();
            firstDispatchRef.current = true;
            setMessages([]);
            setActiveTodos([]);
            setMessageQueue([]);
        } catch (e) {
            console.error('Failed to start new conversation:', e);
        }
    }, [resetStreamingState]);

    const undoTool = useCallback(async (toolCallId: string) => {
        try {
            console.debug('[useChat] Undoing tool batch:', toolCallId);
            const revertedFiles = await invoke<string[]>('undo_batch', { groupId: toolCallId });
            console.debug('[useChat] Reverted files:', revertedFiles);
            // We might want to show a toast or notification here
        } catch (e) {
            console.error('Failed to undo tool batch:', e);
            // Show error in UI?
        }
    }, []);

    return {
        messages,
        loading,
        error,
        sendMessage,
        stopGeneration,
        models,
        refreshModels,
        selectedModelId,
        setSelectedModelId,
        pendingActions,
        approveTool,
        approveToolDecision,
        skipSingleCommand,
        newConversation,
        undoTool,
        setConversation: setMessages,
        loadConversation: useCallback((msgs: ChatMessage[]) => {
            resetStreamingState();
            firstDispatchRef.current = true;
            setMessages(msgs);
            setActiveTodos([]);
            setMessageQueue([]);
        }, [resetStreamingState]),
        toolActivity,
        activeTodos,
        setActiveTodos,
        messageQueue,
        deleteQueuedRequest,
    };
}
