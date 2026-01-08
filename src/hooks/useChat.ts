import { useState, useEffect, useCallback, useRef } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import type { ChatMessage, ModelInfo, ToolCall } from '../types/chat';
import type { Change } from '../types/change';
import { EventNames, type RequestConfirmationPayload, type StructuredAction, type ChangeAppliedPayload, type AllEditsAppliedPayload, type ToolExecutionCompletedPayload } from '../types/events';
import { useEditor } from '../contexts/EditorContext';

export function useChat() {
    const { editorState } = useEditor();
    const [messages, setMessages] = useState<ChatMessage[]>([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

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

        const setupListeners = async () => {
            const u1 = await listen<ChatMessage>('chat-update', (event) => {
                const msg = event.payload;
                console.log('[CHAT UPDATE]', msg);

                setMessages((prev) => {
                    const last = prev[prev.length - 1];

                    // 1. If it's a Tool message (legacy or direct tool output), append it if it's new
                    // Or if we want to hide Tool messages and only show them inside Assistant tool calls, we ignore them
                    // But for now, let's keep the existing logic of using it to update tool status if we can match it.
                    // Actually, with Blade Protocol, the backend sends the UPDATED Assistant message (Role=Assistant)
                    // with status=complete. So we might not need to handle Role=Tool for status updates anymore.

                    if (msg.role === 'Assistant') {
                        // Start loading if this is the start of a response
                        if (!last || last.role !== 'Assistant') {
                            setLoading(true);
                            return [...prev, msg];
                        }

                        // If the last message IS an Assistant message, check if we should Merge or Append.

                        // CASE A: The new message has completed tool calls.
                        // This usually means it's an update to the EXISTING message (e.g. status changed from executing->complete)
                        const isToolUpdate = msg.tool_calls?.some(tc => tc.status === 'complete' || tc.status === 'error');

                        if (isToolUpdate) {
                            // Verify if it matches the last message's tool calls
                            const lastHasMatchingTools = last.tool_calls?.some(ltc =>
                                msg.tool_calls?.some(mtc => mtc.id === ltc.id)
                            );

                            if (lastHasMatchingTools) {
                                // It matches! Replace the last message with this authoritative update
                                console.log('[CHAT] Updating assistant message with completed tools');
                                return [...prev.slice(0, -1), msg];
                            }

                            // If it doesn't match, it might be a NEW response that just happens to have tools immediately?
                            // Or we entered a new turn.
                        }

                        // CASE B: Streaming content update (msg.content is a chunk or full new content)
                        // If the last message already has completed tools, and this new message has content,
                        // it's likely a *new* reasoning/text block AFTER the tool execution.
                        const lastHadCompletedTools = last.tool_calls?.some(tc =>
                            tc.status === 'complete' || tc.status === 'error'
                        );

                        if (lastHadCompletedTools) {
                            // If the previous message was "done" with tools, and we get more content,
                            // usually models generate text -> tool -> tool_result -> MORE text.
                            // In standard Chat (OpenAI), that "MORE text" is a NEW message (Assistant).
                            // OR it's the SAME message being appended to? 
                            // Usually: Assistant(Content + ToolCall) -> Tool(Result) -> Assistant(Content).
                            // So it should be a NEW message.
                            console.log('[CHAT] New AI response after tool completion - appending');
                            return [...prev, msg];
                        }

                        // CASE C: Standard streaming merge (appending content)
                        const mergedToolCalls = msg.tool_calls || last.tool_calls;

                        // Use the new content if provided, else keep old. 
                        // Note: If msg.content is just a chunk, we need to append. 
                        // But Backend `DrainResult::Update` sends chunks? 
                        // Let's check `lib.rs`: `DrainResult::Update(ChatMessage)`. 
                        // `chat_manager.rs` line 1009 `msg.content += chunk`. It accumulates in `updated_assistant_message`.
                        // So `msg` is the FULL ACCUMULATED message if using `updated_assistant_message`, OR just the chunk?
                        // `process_chat_event` in `chat_manager.rs` (lines 600+) accumulates `self.updated_assistant_message`.
                        // AND it emits `DrainResult::Update(self.updated_assistant_message.clone())`.
                        // So `msg` is the FULL message so far. We can just replace.

                        return [...prev.slice(0, -1), msg];

                    } else if (msg.role === 'Tool') {
                        // We still receive Tool messages for history, but we don't need to use them to patch the array
                        // if the Assistant message update handles the UI state.
                        // We just append them to history so the context is correct.
                        return [...prev, msg];
                    } else if (msg.role === 'User') {
                        // Deduplicate user messages (sometimes echoed back)
                        if (last && last.role === 'User' && last.content === msg.content) return prev;
                        return [...prev, msg];
                    }

                    return [...prev, msg];
                });
            });
            unlistenUpdate = u1;

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

            // Tauri command expects snake_case param name
            await BladeDispatcher.workflow({
                type: 'ApproveChange',
                payload: { change_id: changeId }
            });
            console.log('[useChat] Backend approve_change completed');
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
            // Tauri command expects snake_case param name
            await BladeDispatcher.workflow({
                type: 'RejectChange',
                payload: { change_id: changeId }
            });
            setPendingChanges(prev => prev.filter(c => c.id !== changeId));
        } catch (e) {
            console.error("Failed to reject change:", e);
        }
    }, []);

    const approveAllChanges = useCallback(async () => {
        try {
            console.log('[useChat] approveAllChanges called; pendingChanges:', pendingChanges.map(c => ({ id: c.id, path: c.path, type: c.change_type })));
            logFrontend(`[approveAllChanges] count=${pendingChanges.length} ids=${pendingChanges.map(c => c.id).join(',')}`);
            await BladeDispatcher.workflow({
                type: 'ApproveAllChanges',
                payload: {}
            });
            console.log('[useChat] Backend approve_all_changes completed');
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
