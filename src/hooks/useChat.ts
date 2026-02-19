import { useState, useEffect, useCallback, useRef } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import type { ChatMessage, ImageAttachment, ModelInfo, ToolActivityState, ToolCall, StreamingState } from '../types/chat';
import type { Change } from '../types/change';
import { EventNames, type RequestConfirmationPayload, type StructuredAction, type ChangeAppliedPayload, type AllEditsAppliedPayload, type ToolExecutionCompletedPayload } from '../types/events';
import { useEditor } from '../contexts/EditorContext';
import { MessageBuffer } from '../utils/eventBuffer';
import type { BladeEventEnvelope } from '../types/blade';
import { getOrCreateIdempotencyKey, IDEMPOTENT_OPERATIONS } from '../utils/idempotency';
import { ensureMessagesHaveBlocks } from '../utils/messageBlocks';

export function useChat() {
    const { editorState } = useEditor();
    const [messages, setMessages] = useState<ChatMessage[]>([]);
    const messagesRef = useRef<ChatMessage[]>([]);
    const blocksRef = useRef<Map<string, import('../types/chat').MessageBlock[]>>(new Map());
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    
    // Tool activity state for per-tool-call streaming progress display
    const [toolActivity, setToolActivity] = useState<ToolActivityState | null>(null);
    const toolChunkCountsRef = useRef<Map<string, { chunkCount: number; startedAt: number; lastChunkAt: number }>>(new Map());

    // Active todo list state — lifted out of messages for persistent TaskPanel
    const [activeTodos, setActiveTodos] = useState<import('../types/events').TodoItem[]>([]);

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

            pending.forEach((update, id) => {
                const idx = updated.findIndex(m => m.id === id);
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
                    // Create new message - insert after last user message to maintain flow
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
                    
                    // Find the correct insertion point - after the last user message
                    const lastUserIdx = updated.map(m => m.role).lastIndexOf('User');
                    if (lastUserIdx >= 0 && lastUserIdx === updated.length - 1) {
                        // User message is at the end, append after it
                        updated.push(newMsg);
                    } else if (lastUserIdx >= 0) {
                        // Insert after the last user message
                        updated.splice(lastUserIdx + 1, 0, newMsg);
                    } else {
                        // No user message found, append at end
                        updated.push(newMsg);
                    }
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
            console.log('[useChat] Synced model to backend:', modelId);
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
                    console.log('Not in Tauri environment, skipping chat init');
                    return;
                }

                const [history, modelList, isStreaming] = await Promise.all([
                    invoke<ChatMessage[]>('get_conversation'),
                    invoke<ModelInfo[]>('list_models'),
                    invoke<boolean>('get_chat_status'),
                ]);

                console.log('Loaded conversation:', history);
                // Reconstruct blocks for historical messages
                setMessages(ensureMessagesHaveBlocks(history));
                setModels(modelList);

                // Restore loading state if backend is still streaming (e.g. after UI reload)
                if (isStreaming) {
                    console.log('[useChat] Backend is still streaming — restoring loading state');
                    setLoading(true);
                }

                // Set a default model - project state will override this if available
                // This prevents the model from being undefined before project state loads
                if (modelList.length > 0 && !hasExplicitModelRef.current) {
                    const defaultModel = modelList.find(m => m.id === 'anthropic/claude-sonnet-4-5-20250929')
                        || modelList.find(m => m.id === 'openai/gpt-5.2')
                        || modelList[0];
                    setSelectedModelIdState(defaultModel.id);
                    console.log('[useChat] Set initial default model:', defaultModel.id);
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
                        accumulatedReasoningRef.current.content += chunk;
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
                        accumulatedContentRef.current.content += chunk;
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
                        // Create new reasoning block if:
                        // 1. No blocks exist yet
                        // 2. Last block is not reasoning (text or tool_call)
                        // This ensures reasoning after tool calls gets its own block
                        if (lastBlock && lastBlock.type === 'reasoning') {
                            // Append to existing reasoning block (continuous reasoning)
                            blocks[blocks.length - 1] = { ...lastBlock, content: lastBlock.content + chunk };
                        } else {
                            // Create new reasoning block (after text, tool_call, or first block)
                            blocks = [...blocks, { type: 'reasoning', content: chunk, id: crypto.randomUUID() }];
                        }
                    } else {
                        if (lastBlock && lastBlock.type === 'text') {
                            // Append to existing text block
                            blocks[blocks.length - 1] = { ...lastBlock, content: lastBlock.content + chunk };
                        } else {
                            // Create new text block
                            blocks = [...blocks, { type: 'text', content: chunk, id: crypto.randomUUID() }];
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
                console.log('[CHAT UPDATE]', msg);
                // ... legacy logic ...
                 setMessages((prev) => {
                     // ... 
                     return prev;
                 });
            });
            unlistenUpdate = u1;
            */

            const u2 = await listen('chat-done', () => {
                setLoading(false);
                setPendingActions(null); // Clear any hanging dialogs

                // Auto-complete lingering todos when chat finishes.
                // Models sometimes forget to send a final todo_write marking the last task as completed.
                // Wait briefly to allow any in-flight todo_updated events to arrive first.
                setTimeout(() => {
                    setActiveTodos(prev => {
                        if (prev.length === 0) return prev;
                        const hasIncomplete = prev.some(t => t.status !== 'completed');
                        if (!hasIncomplete) return prev; // Already all completed, normal flow handles it
                        // Mark all remaining items as completed
                        const completed = prev.map(t => ({ ...t, status: 'completed' as const }));
                        // Trigger the completion flow (clear panel + insert summary) after brief display
                        setTimeout(() => {
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
                        return completed;
                    });
                }, 500);
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
                console.log('[useChat] Context length exceeded:', event.payload);
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
                console.log('[useChat] Message too large:', event.payload);
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
                console.log("Permission requested for:", event.payload);
                setPendingActions(event.payload.actions);
            });
            unlistenPerm = u4;



            // Listen for command executions
            const u6 = await listen<{ command: string; cwd?: string; output: string; exitCode: number; duration?: number; call_id: string }>('command-executed', (event) => {
                const { command, cwd, output, exitCode, duration, call_id } = event.payload;
                console.log('[COMMAND EXECUTED]', { command, call_id, exitCode });

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

            // u7 removed - redundant with chat-update logic



            // Listen for todo list updates — update shared state for TaskPanel
            const u10 = await listen<{ todos: import('../types/events').TodoItem[] }>(EventNames.TODO_UPDATED, (event) => {
                const todos = event.payload.todos;
                invoke('log_frontend', { message: `[FRONTEND] TODO_UPDATED received: ${todos.length} items` });
                setActiveTodos(todos);

                // Completion detection: all tasks done → brief "all done" state → hide panel + insert summary
                const allCompleted = todos.length > 0 && todos.every(t => t.status === 'completed');
                if (allCompleted) {
                    setTimeout(() => {
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
                            setTimeout(() => {
                                setToolActivity(prev => {
                                    if (!prev) return prev;
                                    const prevKey = prev.toolCallId || `${prev.toolName}:${prev.filePath}`;
                                    return prevKey === key ? null : prev;
                                });
                            }, 2000);
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
                // if (unlistenToolCompleted) unlistenToolCompleted(); // Removed
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
        };
    }, [queueMessageUpdate, flushPendingUpdates]);

    const [messageQueue, setMessageQueue] = useState<{ text: string; attachments?: ImageAttachment[] }[]>([]);

    const dispatchToBackend = useCallback(async (text: string, attachments?: ImageAttachment[]) => {
        try {
            setLoading(true);
            setError(null);

            // Get editor state from context
            const activeFile = editorState.activeFile;
            // activeFile might be null/undefined, ensure we pass string or null
            const safeActiveFile = activeFile || null;
            const openFiles = editorState.openFiles.length > 0
                ? editorState.openFiles
                : (activeFile ? [activeFile] : []);

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
                    context: {
                        active_file: safeActiveFile, // Use active tab file as context
                        open_files: openFiles,
                        cursor_line: editorState.cursorLine ?? null,
                        cursor_column: editorState.cursorColumn ?? null,
                        selection_start: editorState.selectionStartLine ?? null,
                        selection_end: editorState.selectionEndLine ?? null
                    }
                }
            });

        } catch (e) {
            console.error('Failed to send message:', e);
            setError(e instanceof Error ? e.message : String(e));
            setLoading(false); // Ensure loading is cleared on immediate error
        }
    }, [
        editorState.activeFile,
        editorState.openFiles,
        editorState.cursorLine,
        editorState.cursorColumn,
        editorState.selectionStartLine,
        editorState.selectionEndLine,
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
    const stopGeneration = useCallback(async () => {
        try {
            await BladeDispatcher.chat({ type: 'StopGeneration', payload: {} });
            setLoading(false);
            // Clear any pending command approvals when stopping
            setPendingActions(null);
        } catch (e) {
            console.error("Failed to stop generation:", e);
        }
    }, []);



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
    }, []);

    const newConversation = useCallback(async () => {
        try {
            await BladeDispatcher.chat({
                type: 'NewConversation',
                payload: { model: selectedModelIdRef.current }
            });
            resetStreamingState();
            setMessages([]);
            setActiveTodos([]);
        } catch (e) {
            console.error('Failed to start new conversation:', e);
        }
    }, [resetStreamingState]);

    const undoTool = useCallback(async (toolCallId: string) => {
        try {
            console.log('[useChat] Undoing tool batch:', toolCallId);
            const revertedFiles = await invoke<string[]>('undo_batch', { groupId: toolCallId });
            console.log('[useChat] Reverted files:', revertedFiles);
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
        newConversation,
        undoTool,
        setConversation: setMessages,
        loadConversation: useCallback((msgs: ChatMessage[]) => {
            resetStreamingState();
            setMessages(msgs);
            setActiveTodos([]);
        }, [resetStreamingState]),
        toolActivity,
        activeTodos,
        setActiveTodos,
    };
}
