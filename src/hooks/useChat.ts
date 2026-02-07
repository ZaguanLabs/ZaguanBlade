import { useState, useEffect, useCallback, useRef } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import type { ChatMessage, ImageAttachment, ModelInfo, ToolCall } from '../types/chat';
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
    
    // Tool activity state for streaming progress display
    const [toolActivity, setToolActivity] = useState<{ toolName: string; filePath: string; action: string } | null>(null);

    // v1.1: Message buffer and accumulation ref for atomic updates
    const messageBufferRef = useRef<MessageBuffer | null>(null);
    const accumulatedContentRef = useRef<{ id: string; content: string }>({ id: '', content: '' });
    const accumulatedReasoningRef = useRef<{ id: string; content: string }>({ id: '', content: '' });

    // v1.2: Batched rendering - buffer updates and flush at intervals
    // This prevents re-rendering on every single streaming chunk
    const pendingUpdatesRef = useRef<Map<string, { content: string; reasoning: string; blocks: import('../types/chat').MessageBlock[] }>>(new Map());
    const flushScheduledRef = useRef<number | null>(null);
    const FLUSH_INTERVAL_MS = 50; // 20fps - smooth enough for human perception

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
                    if (msg.content !== update.content || msg.reasoning !== update.reasoning) {
                        if (!changed) {
                            updated = [...prev];
                            changed = true;
                        }
                        // Merge blocks: keep tool_call/command_execution/todo/research blocks from existing message,
                        // replace text/reasoning blocks with new ones from the buffer
                        const existingNonTextBlocks = (msg.blocks || []).filter(
                            b => b.type !== 'text' && b.type !== 'reasoning'
                        );
                        const newTextBlocks = update.blocks.filter(
                            b => b.type === 'text' || b.type === 'reasoning'
                        );
                        // Natural conversation flow: tool calls first, then response text after
                        // This matches how the model actually works - it calls tools, gets results, then responds
                        const mergedBlocks = [...existingNonTextBlocks, ...newTextBlocks];
                        
                        // Debug: log if we have todos
                        if (msg.todos && msg.todos.length > 0) {
                            console.log(`[flushPendingUpdates] Preserving ${msg.todos.length} todos for message ${id}, has todo block: ${existingNonTextBlocks.some(b => b.type === 'todo')}`);
                        }
                        
                        updated[idx] = {
                            ...msg,
                            content: update.content,
                            reasoning: update.reasoning,
                            blocks: mergedBlocks,
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
    const queueMessageUpdate = useCallback((id: string, content: string, reasoning: string, blocks: import('../types/chat').MessageBlock[]) => {
        pendingUpdatesRef.current.set(id, { content, reasoning, blocks });
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
            await invoke('set_selected_model', { modelId });
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

                const [history, modelList] = await Promise.all([
                    invoke<ChatMessage[]>('get_conversation'),
                    invoke<ModelInfo[]>('list_models'),
                ]);

                console.log('Loaded conversation:', history);
                // Reconstruct blocks for historical messages
                setMessages(ensureMessagesHaveBlocks(history));
                setModels(modelList);

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
                (id, chunk, is_final, type) => {
                    setLoading(true);

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
                            // New content stream for this message - clear stale text blocks from blocksRef
                            // Keep only non-text blocks (tool_call, command_execution, etc.)
                            const existingBlocks = blocksRef.current.get(id) || [];
                            if (existingBlocks.length > 0) {
                                const nonTextBlocks = existingBlocks.filter(b => b.type !== 'text' && b.type !== 'reasoning');
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
                        blocks
                    );
                },
                (id) => {
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
                    const newBlocks = [...(msg.blocks || [])];

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



            // Listen for todo list updates
            const u10 = await listen<{ todos: import('../types/events').TodoItem[] }>(EventNames.TODO_UPDATED, (event) => {
                invoke('log_frontend', { message: `[FRONTEND] TODO_UPDATED received: ${event.payload.todos.length} items` });
                setMessages((prev) => {
                    const updated = [...prev];
                    // Find the last assistant message and attach the todos
                    let found = false;
                    for (let i = updated.length - 1; i >= 0; i--) {
                        if (updated[i].role === 'Assistant') {
                            const msg = updated[i];
                            const newBlocks = [...(msg.blocks || [])];

                            // Check if we already have a todo block - update it if so, otherwise add one
                            const existingTodoBlockIdx = newBlocks.findIndex(b => b.type === 'todo');
                            const todoBlockId = existingTodoBlockIdx >= 0
                                ? newBlocks[existingTodoBlockIdx].id
                                : crypto.randomUUID();

                            if (existingTodoBlockIdx < 0) {
                                // Add new todo block at current position in the conversation flow
                                newBlocks.push({ type: 'todo' as const, id: todoBlockId });
                            }

                            // CRITICAL: Also update blocksRef to prevent message buffer from overwriting
                            if (msg.id) {
                                const currentBlocksRef = blocksRef.current.get(msg.id) || [];
                                if (!currentBlocksRef.some(b => b.type === 'todo')) {
                                    blocksRef.current.set(msg.id, [...currentBlocksRef, { type: 'todo' as const, id: todoBlockId }]);
                                }
                            }

                            updated[i] = {
                                ...msg,
                                todos: event.payload.todos,
                                blocks: newBlocks
                            };
                            found = true;
                            invoke('log_frontend', { message: `[FRONTEND] Attached todos to message at index ${i}, message has ${updated[i].todos?.length} todos, blocks: ${JSON.stringify(newBlocks.map(b => b.type))}` });
                            break;
                        }
                    }
                    if (!found) {
                        invoke('log_frontend', { message: `[FRONTEND] No assistant message found to attach todos! Messages: ${updated.length}` });
                    }
                    return updated;
                });
            });
            const unlistenTodoUpdated = u10;

            // v1.1: blade-event listener for MessageDelta with sequence numbers
            const u11 = await listen<BladeEventEnvelope>('blade-event', (event) => {
                const envelope = event.payload;

                if (envelope.event.type === 'Chat') {
                    const chatEvent = envelope.event.payload;

                    if (chatEvent.type === 'MessageDelta') {
                        const { id, seq, chunk, is_final } = chatEvent.payload;
                        console.log(`[v1.1 Chat] MessageDelta: id=${id}, seq=${seq}, is_final=${is_final}, chunk_len=${chunk.length}`);

                        // Use buffer to handle out-of-order chunks
                        if (messageBufferRef.current) {
                            messageBufferRef.current.addMessageDelta(id, seq, chunk, is_final);
                        }
                    } else if (chatEvent.type === 'ReasoningDelta') {
                        const { id, seq, chunk, is_final } = chatEvent.payload;
                        console.log(`[v1.1 Chat] ReasoningDelta: id=${id}, seq=${seq}, is_final=${is_final}, chunk_len=${chunk.length}, chunk="${chunk.slice(0, 50)}..."`);

                        if (messageBufferRef.current) {
                            messageBufferRef.current.addReasoningDelta(id, seq, chunk, is_final);
                        } else {
                            console.warn('[v1.1 Chat] ReasoningDelta received but messageBufferRef is null!');
                        }
                    } else if (chatEvent.type === 'MessageCompleted') {
                        const { id } = chatEvent.payload;
                        console.log(`[v1.1 Chat] MessageCompleted: id=${id}`);

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
                        setToolActivity(null);
                        // Buffer will auto-clear on is_final, but this provides explicit confirmation
                    } else if (chatEvent.type === 'ToolUpdate') {
                        const { message_id, tool_call_id, status, result, tool_call } = chatEvent.payload;
                        console.log(`[v1.1 Chat] ToolUpdate: msg=${message_id} tool=${tool_call_id} status=${status}`);

                        // Clear the tool activity preview — the real ToolCallDisplay takes over
                        setToolActivity(null);

                        setMessages(prev => {
                            const existingIdx = prev.findIndex(msg => msg.id === message_id);

                            if (existingIdx === -1) {
                                // Create new message for tool if missing
                                const newMsg: ChatMessage = {
                                    id: message_id,
                                    role: 'Assistant',
                                    content: '',
                                    tool_calls: tool_call ? [{ ...tool_call, status: status as any, result }] : [],
                                    blocks: tool_call ? [{ type: 'tool_call', id: tool_call_id }] : []
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
                        // Handle tool activity events (including streaming progress)
                        const { tool_name, file_path, action } = chatEvent.payload;
                        console.log(`[v1.1 Chat] ToolActivity: ${tool_name} -> ${file_path} (${action})`);
                        
                        // Update tool activity state for UI display
                        setToolActivity({
                            toolName: tool_name,
                            filePath: file_path,
                            action: action
                        });
                        
                        // Clear tool activity after a short delay if action is not "streaming"
                        // For streaming, it will be cleared when the actual tool call arrives
                        if (action !== 'streaming') {
                            setTimeout(() => {
                                setToolActivity(prev => {
                                    // Only clear if it's still the same activity
                                    if (prev?.toolName === tool_name && prev?.filePath === file_path) {
                                        return null;
                                    }
                                    return prev;
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
            const openFiles = activeFile ? [activeFile] : [];

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
        editorState.cursorLine,
        editorState.cursorColumn,
        editorState.selectionStartLine,
        editorState.selectionEndLine,
    ]);

    // Queue processing effect
    useEffect(() => {
        console.log('[TRIPWIRE] Queue effect - loading:', loading, 'queueLength:', messageQueue.length);
        if (!loading && messageQueue.length > 0) {
            const nextMessage = messageQueue[0];
            console.log('[TRIPWIRE] Processing queued message:', nextMessage.text.substring(0, 50));
            setMessageQueue(prev => prev.slice(1));
            dispatchToBackend(nextMessage.text, nextMessage.attachments);
        }
    }, [loading, messageQueue, dispatchToBackend]);

    const sendMessage = useCallback((text: string, attachments?: ImageAttachment[]) => {
        console.log('[TRIPWIRE] sendMessage called - loading:', loading, 'text:', text.substring(0, 50));
        // Optimistically add user message
        const userMsg: ChatMessage = {
            id: crypto.randomUUID(),
            role: 'User',
            content: text,
            images: attachments
        };
        setMessages(prev => [...prev, userMsg]);

        // Add to queue for processing
        console.log('[TRIPWIRE] Adding message to queue');
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
            await invoke('new_conversation', { modelId: selectedModelIdRef.current });
            setMessages([]);
            setLoading(false);
            setPendingActions(null);
        } catch (e) {
            console.error('Failed to start new conversation:', e);
        }
    }, []);

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
        toolActivity,
    };
}
