import { useState, useEffect, useCallback, useRef } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import type { ChatMessage, ModelInfo, ToolCall } from '../types/chat';
import type { Change } from '../types/change';
import { EventNames, type RequestConfirmationPayload, type StructuredAction, type ChangeAppliedPayload, type AllEditsAppliedPayload, type ToolExecutionCompletedPayload } from '../types/events';
import { useEditor } from '../contexts/EditorContext';
import { MessageBuffer } from '../utils/eventBuffer';
import type { BladeEventEnvelope } from '../types/blade';
import { getOrCreateIdempotencyKey, IDEMPOTENT_OPERATIONS } from '../utils/idempotency';

export function useChat() {
    const { editorState } = useEditor();
    const [messages, setMessages] = useState<ChatMessage[]>([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    // v1.1: Message buffer and accumulation ref for atomic updates
    const messageBufferRef = useRef<MessageBuffer | null>(null);
    const accumulatedContentRef = useRef<{ id: string; content: string }>({ id: '', content: '' });
    const accumulatedReasoningRef = useRef<{ id: string; content: string }>({ id: '', content: '' });

    const [models, setModels] = useState<ModelInfo[]>([]);
    const [selectedModelId, setSelectedModelId] = useState<string>('anthropic/claude-sonnet-4-5-20250929');

    const logFrontend = useCallback(async (message: string) => {
        try {
            await invoke('log_frontend', { message });
        } catch (e) {
            console.error('[useChat] log_frontend failed', e);
        }
    }, []);

    // Permission Logic
    const [pendingActions, setPendingActions] = useState<StructuredAction[] | null>(null);
    const [pendingChanges, setPendingChanges] = useState<Change[]>([]);

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
                    invoke<ModelInfo[]>('list_models')
                ]);

                console.log('Loaded conversation:', history);
                setMessages(history);
                setModels(modelList);

                if (modelList.length > 0) {
                    const dafault = modelList.find(m => m.id === 'anthropic/claude-sonnet-4-5-20250929')
                        || modelList.find(m => m.id === 'openai/gpt-5.2')
                        || modelList[0];
                    setSelectedModelId(dafault.id);
                }

            } catch (e) {
                console.error('Failed to init:', e);
                // Don't show error if it's just because backend isn't ready or we are server-side
            }
        }
        init();
    }, []);

    // Listen for updates
    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;

        let unlistenUpdate: (() => void) | undefined;
        let unlistenDone: (() => void) | undefined;
        let unlistenError: (() => void) | undefined;
        let unlistenPerm: (() => void) | undefined;
        let unlistenChanges: (() => void) | undefined;
        let unlistenCommand: (() => void) | undefined;
        let unlistenToolCompleted: (() => void) | undefined;
        let unlistenV11: (() => void) | undefined;

        // Initialize v1.1 message buffer
        if (!messageBufferRef.current) {
            messageBufferRef.current = new MessageBuffer(
                (id, chunk, is_final, type) => {
                    // console.log(`[v1.1 MessageBuffer] Chunk ${id} len=${chunk.length} type=${type}`);
                    setLoading(true);

                    if (type === 'reasoning') {
                        if (accumulatedReasoningRef.current.id !== id) {
                            accumulatedReasoningRef.current = { id, content: '' };
                        }
                        accumulatedReasoningRef.current.content += chunk;
                        const fullReasoning = accumulatedReasoningRef.current.content;

                        setMessages((prev) => {
                            const existingIdx = prev.findIndex(m => m.id === id);
                            if (existingIdx !== -1) {
                                const updated = [...prev];
                                updated[existingIdx] = { ...updated[existingIdx], reasoning: fullReasoning };
                                return updated;
                            }
                            return [...prev, { id, role: 'Assistant', reasoning: fullReasoning, content: '' } as ChatMessage];
                        });
                    } else {
                        // Regular Content
                        if (accumulatedContentRef.current.id !== id) {
                            accumulatedContentRef.current = { id, content: '' };
                        }
                        accumulatedContentRef.current.content += chunk;
                        const fullContent = accumulatedContentRef.current.content;

                        setMessages((prev) => {
                            const existingIdx = prev.findIndex(m => m.id === id);
                            if (existingIdx !== -1) {
                                const updated = [...prev];
                                updated[existingIdx] = { ...updated[existingIdx], content: fullContent };
                                return updated;
                            }
                            const last = prev[prev.length - 1];
                            if (last && last.role === 'Assistant' && !last.id) {
                                const updated = [...prev];
                                updated[prev.length - 1] = { ...last, id, content: fullContent };
                                return updated;
                            }
                            return [...prev, { id, role: 'Assistant', content: fullContent } as ChatMessage];
                        });
                    }
                },
                (id) => {
                    console.log(`[v1.1 MessageBuffer] Message ${id} completed`);
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
                setError(event.payload);
                setLoading(false);
                setPendingActions(null);
            });
            unlistenError = u3;

            // Listen for permission requests
            const u4 = await listen<RequestConfirmationPayload>('request-confirmation', (event) => {
                console.log("Permission requested for:", event.payload);
                setPendingActions(event.payload.actions);
            });
            unlistenPerm = u4;

            // Listen for change proposals (backend sends array of changes)
            const u5 = await listen<Change[]>('propose-changes', (event) => {
                console.log('[PROPOSE CHANGES] received', event.payload.map(c => ({ id: c.id, type: c.change_type, path: c.path })));
                setPendingChanges(prev => {
                    // Filter out duplicates
                    const newChanges = event.payload.filter(newChange =>
                        !prev.some(c => c.id === newChange.id)
                    );
                    return [...prev, ...newChanges];
                });

                // Automatically open new files as ephemeral tabs
                event.payload.forEach(change => {
                    if (change.change_type === 'new_file') {
                        const filename = change.path.split('/').pop() || change.path;
                        emit('open-ephemeral-document', {
                            id: `new-file-${change.id}`,
                            title: `NEW: ${filename}`,
                            content: change.content,
                            suggestedName: change.path
                        });
                    }
                });
            });
            unlistenChanges = u5;

            // Listen for command executions
            const u6 = await listen<{ command: string; cwd?: string; output: string; exitCode: number; duration?: number }>('command-executed', (event) => {
                console.log('[COMMAND EXECUTED]', event.payload);
                setMessages(prev => {
                    const lastAssistantIndex = prev.findIndex((m, i) =>
                        m.role === 'Assistant' && i === prev.length - 1
                    );
                    if (lastAssistantIndex === -1) return prev;

                    const updated = [...prev];
                    const msg = updated[lastAssistantIndex];
                    updated[lastAssistantIndex] = {
                        ...msg,
                        commandExecutions: [
                            ...(msg.commandExecutions || []),
                            {
                                command: event.payload.command,
                                cwd: event.payload.cwd,
                                output: event.payload.output,
                                exitCode: event.payload.exitCode,
                                duration: event.payload.duration,
                                timestamp: Date.now(),
                            },
                        ],
                    };
                    return updated;
                });
            });
            unlistenCommand = u6;

            // u7 removed - redundant with chat-update logic

            // Listen for change applied signal (from individual or batch approval)
            const u8 = await listen<ChangeAppliedPayload>(EventNames.CHANGE_APPLIED, (event) => {
                const { change_id, file_path } = event.payload;
                console.log('[CHAT] Change applied signal received:', change_id, 'for', file_path);
                setPendingChanges(prev => prev.filter(c => c.id !== change_id && c.path !== file_path));
            });
            const unlistenApplied = u8;

            const u9 = await listen<AllEditsAppliedPayload>(EventNames.ALL_EDITS_APPLIED, (event) => {
                const { file_paths } = event.payload;
                console.log('[CHAT] All changes applied for:', file_paths);
                setPendingChanges(prev => prev.filter(c => !file_paths.includes(c.path)));
            });
            const unlistenAllApplied = u9;

            // Listen for todo list updates
            const u10 = await listen<{ todos: import('../types/events').TodoItem[] }>(EventNames.TODO_UPDATED, (event) => {
                console.log('[TODO UPDATED]', event.payload);
                setMessages((prev) => {
                    const updated = [...prev];
                    // Find the last assistant message and attach the todos
                    for (let i = updated.length - 1; i >= 0; i--) {
                        if (updated[i].role === 'Assistant') {
                            updated[i] = {
                                ...updated[i],
                                todos: event.payload.todos
                            };
                            break;
                        }
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
                        console.log(`[v1.1 Chat] ReasoningDelta: id=${id}, seq=${seq}, is_final=${is_final}, chunk_len=${chunk.length}`);

                        if (messageBufferRef.current) {
                            messageBufferRef.current.addReasoningDelta(id, seq, chunk, is_final);
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
                        // Buffer will auto-clear on is_final, but this provides explicit confirmation
                    } else if (chatEvent.type === 'ToolUpdate') {
                        const { message_id, tool_call_id, status, result, tool_call } = chatEvent.payload;
                        console.log(`[v1.1 Chat] ToolUpdate: msg=${message_id} tool=${tool_call_id} status=${status}`);

                        setMessages(prev => {
                            const existingIdx = prev.findIndex(msg => msg.id === message_id);

                            if (existingIdx === -1) {
                                console.log('[v1.1 Chat] Creating missing assistant message for tool call:', message_id);
                                const newMsg: ChatMessage = {
                                    id: message_id,
                                    role: 'Assistant',
                                    content: '',
                                    tool_calls: tool_call ? [{ ...tool_call, status: status as any, result }] : []
                                };
                                return [...prev, newMsg];
                            }

                            return prev.map(msg => {
                                if (msg.id === message_id) {
                                    const existingTools = msg.tool_calls || [];
                                    const toolIndex = existingTools.findIndex(tc => tc.id === tool_call_id);

                                    if (toolIndex >= 0) {
                                        // Update existing
                                        const newTools = [...existingTools];
                                        newTools[toolIndex] = { ...newTools[toolIndex], status: status as any };
                                        if (result) newTools[toolIndex].result = result;
                                        // Update properties if provided (e.g. arguments might change or full details arrived)
                                        if (tool_call) {
                                            newTools[toolIndex] = { ...newTools[toolIndex], ...tool_call };
                                        }
                                        return { ...msg, tool_calls: newTools };
                                    } else {
                                        // Add new tool call
                                        if (tool_call) {
                                            console.log('[v1.1 Chat] Adding new tool call:', tool_call);
                                            // Snapshot content as "before tools" if this is the first tool call appearing
                                            // This allows the UI to compute post-tool text by diffing valid content vs content_before_tools
                                            const contentBefore = msg.content_before_tools !== undefined ? msg.content_before_tools : msg.content;
                                            return {
                                                ...msg,
                                                content_before_tools: contentBefore,
                                                tool_calls: [...existingTools, tool_call]
                                            };
                                        } else {
                                            console.warn('[v1.1 Chat] Received ToolUpdate for unknown tool but no tool_call data provided:', tool_call_id);
                                        }
                                    }
                                }
                                return msg;
                            });
                        });
                    }
                }
            });
            unlistenV11 = u11;

            return () => {
                if (unlistenUpdate) unlistenUpdate();
                if (unlistenDone) unlistenDone();
                if (unlistenError) unlistenError();
                if (unlistenPerm) unlistenPerm();
                if (unlistenChanges) unlistenChanges();
                if (unlistenCommand) unlistenCommand();
                // if (unlistenToolCompleted) unlistenToolCompleted(); // Removed
                if (unlistenApplied) unlistenApplied();
                if (unlistenAllApplied) unlistenAllApplied();
                if (unlistenTodoUpdated) unlistenTodoUpdated();
                if (unlistenV11) unlistenV11();
            };
        };

        const cleanupPromise = setupListeners();

        return () => {
            cleanupPromise.then(cleanup => cleanup());
        };
    }, []);

    const sendMessage = useCallback(async (text: string) => {
        try {
            setLoading(true);
            setError(null);

            // Optimistically add user message
            const userMsg: ChatMessage = {
                id: crypto.randomUUID(),
                role: 'User',
                content: text
            };
            setMessages(prev => [...prev, userMsg]);

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
                    model: selectedModelId,
                    context: {
                        active_file: safeActiveFile,
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
        }
    }, [
        editorState.activeFile,
        editorState.cursorLine,
        editorState.cursorColumn,
        editorState.selectionStartLine,
        editorState.selectionEndLine,
        selectedModelId
    ]);
    const stopGeneration = useCallback(async () => {
        try {
            await BladeDispatcher.chat({ type: 'StopGeneration' });
            setLoading(false);
            // Clear any pending command approvals when stopping
            setPendingActions(null);
        } catch (e) {
            console.error("Failed to stop generation:", e);
        }
    }, []);

    const approveChange = useCallback(async (changeId: string) => {
        console.log('[useChat] approveChange called with:', changeId);
        logFrontend(`[approveChange] changeId=${changeId}`);
        try {
            // Get the change being approved to check its file path
            const change = pendingChanges.find(c => c.id === changeId);
            console.log('[useChat] Found change:', change);
            logFrontend(`[approveChange] found change path=${change?.path ?? 'n/a'} type=${change?.change_type ?? 'n/a'}`);

            // v1.1: Generate idempotency key for this critical operation
            const idempotencyKey = getOrCreateIdempotencyKey(
                IDEMPOTENT_OPERATIONS.APPROVE_CHANGE,
                changeId
            );

            // Use v1.1 ApproveAction intent with idempotency
            await BladeDispatcher.dispatch(
                'Workflow',
                { type: 'Workflow', payload: { type: 'ApproveAction', payload: { action_id: changeId } } },
                idempotencyKey
            );
            console.log('[useChat] Backend approve_action completed');
            logFrontend(`[approveChange] backend completed changeId=${changeId}`);

            // Remove the approved change and any other changes for the same file
            // (since applying one patch invalidates others for the same file)
            setPendingChanges(prev => {
                if (change) {
                    const filtered = prev.filter(c => c.id !== changeId && c.path !== change.path);
                    if (filtered.length < prev.length - 1) {
                        console.log(`[CHANGE] Removed ${prev.length - filtered.length - 1} stale changes for ${change.path}`);
                    }
                    return filtered;
                }
                return prev.filter(c => c.id !== changeId);
            });
        } catch (e) {
            console.error("Failed to approve change:", e);
        }
    }, [pendingChanges, logFrontend]);

    const rejectChange = useCallback(async (changeId: string) => {
        try {
            // v1.1: Generate idempotency key for this critical operation
            const idempotencyKey = getOrCreateIdempotencyKey(
                IDEMPOTENT_OPERATIONS.REJECT_CHANGE,
                changeId
            );

            // Use v1.1 RejectAction intent with idempotency
            await BladeDispatcher.dispatch(
                'Workflow',
                { type: 'Workflow', payload: { type: 'RejectAction', payload: { action_id: changeId } } },
                idempotencyKey
            );
            setPendingChanges(prev => prev.filter(c => c.id !== changeId));
        } catch (e) {
            console.error("Failed to reject change:", e);
        }
    }, []);

    const approveAllChanges = useCallback(async () => {
        try {
            console.log('[useChat] approveAllChanges called; pendingChanges:', pendingChanges.map(c => ({ id: c.id, path: c.path, type: c.change_type })));
            logFrontend(`[approveAllChanges] count=${pendingChanges.length} ids=${pendingChanges.map(c => c.id).join(',')}`);

            // v1.1: Generate idempotency key using batch ID (timestamp-based)
            const batchId = `batch-${Date.now()}`;
            const idempotencyKey = getOrCreateIdempotencyKey(
                IDEMPOTENT_OPERATIONS.APPROVE_ALL,
                batchId
            );

            // Use v1.1 ApproveAll intent with idempotency
            await BladeDispatcher.dispatch(
                'Workflow',
                { type: 'Workflow', payload: { type: 'ApproveAll', payload: { batch_id: batchId } } },
                idempotencyKey
            );
            console.log('[useChat] Backend approve_all completed');
            logFrontend('[approveAllChanges] backend completed');
            setPendingChanges([]);
        } catch (e) {
            console.error("Failed to approve all changes:", e);
        }
    }, [pendingChanges, logFrontend]);

    const removeChangesFromList = useCallback((changeIds: string[]) => {
        setPendingChanges(prev => prev.filter(c => !changeIds.includes(c.id)));
    }, []);

    const approveTool = useCallback(async (approved: boolean) => {
        try {
            await BladeDispatcher.workflow({
                type: 'ApproveTool',
                payload: { approved }
            });
            setPendingActions(null);
        } catch (e) {
            console.error('Failed to approve tool:', e);
        }
    }, []);

    const approveToolDecision = useCallback(async (decision: string) => {
        try {
            await BladeDispatcher.workflow({
                type: 'ApproveToolDecision',
                payload: { decision }
            });
            setPendingActions(null);
        } catch (e) {
            console.error('Failed to approve tool decision:', e);
        }
    }, []);

    return {
        messages,
        loading,
        error,
        sendMessage,
        stopGeneration,
        models,
        selectedModelId,
        setSelectedModelId,
        pendingActions,
        approveTool,
        approveToolDecision,
        pendingChanges,
        approveChange,
        approveAllChanges,
        rejectChange,
        removeChangesFromList
    };
}
