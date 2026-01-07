import { useState, useEffect, useCallback, useRef } from 'react';
import { listen, emit } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
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
    const [pendingActionErrors, setPendingActionErrors] = useState<string | null>(null);
    const pendingActionsRef = useRef<StructuredAction[] | null>(null);
    const [pendingToolCallId, setPendingToolCallId] = useState<string | null>(null);
    const [pendingBatchId, setPendingBatchId] = useState<string | null>(null);
    const [pendingTools, setPendingTools] = useState<any[]>([]);
    const [pendingActionsLoading, setPendingActionsLoading] = useState(false);

    // Track if we've created a research tab for current message
    const researchTabCreatedRef = useRef(false);

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
                    
                    // Only set loading if we're adding a new assistant message (streaming started)
                    if (!last || last.role !== 'Assistant') {
                        setLoading(true);
                    }
                    
                    // Handle Tool messages - match them with tool calls in previous assistant message
                    if (msg.role === 'Tool' && msg.tool_call_id) {
                        console.log('[TOOL RESULT] Matching tool_call_id:', msg.tool_call_id);
                        // Find the assistant message with this tool call and update its status
                        const updated = prev.map((m) => {
                            if (m.role === 'Assistant' && m.tool_calls) {
                                console.log('[TOOL RESULT] Checking assistant message with tool calls:', m.tool_calls.map(c => c.id));
                                const updatedCalls = m.tool_calls.map(call => {
                                    if (call.id === msg.tool_call_id) {
                                        console.log('[TOOL RESULT] MATCH! Updating status to complete');
                                        return {
                                            ...call,
                                            status: 'complete' as const,
                                            result: msg.content
                                        };
                                    }
                                    return call;
                                });
                                return { ...m, tool_calls: updatedCalls };
                            }
                            return m;
                        });
                        console.log('[TOOL RESULT] Updated messages:', updated);
                        // Don't add the Tool message itself - it's shown in the tool call display
                        return updated;
                    }
                    
                    if (last && last.role === msg.role && last.role === 'Assistant') {
                        // Only update if this is a streaming continuation (no tool_calls completed yet)
                        // If the last message has completed tool calls, this is a NEW response, so append
                        const hasCompletedTools = last.tool_calls?.some(tc => 
                            tc.status === 'complete' || tc.status === 'error' || tc.status === 'skipped'
                        );
                        
                        if (hasCompletedTools) {
                            // This is a new AI response after tool execution - append it
                            console.log('[CHAT] New AI response after tool completion - appending');
                            return [...prev, msg];
                        }
                        
                        // This is a streaming update to the current message - merge it
                        const mergedToolCalls = msg.tool_calls || last.tool_calls;
                        const updatedToolCalls = mergedToolCalls?.map(call => ({
                            ...call,
                            status: call.status || 'executing' as const
                        }));
                        
                        const finalContent = msg.content !== undefined ? msg.content : last.content;
                        
                        const updatedMsg = {
                            ...last,
                            content: finalContent,
                            tool_calls: updatedToolCalls,
                            reasoning: msg.reasoning || last.reasoning,
                            progress: msg.progress || last.progress
                        };
                        
                        return [...prev.slice(0, -1), updatedMsg];
                    } else if (last && last.role === msg.role && last.role === 'User') {
                        if (last.content === msg.content) return prev;
                        return [...prev, msg];
                    }
                    return [...prev, msg];
                });
            });
            unlistenUpdate = u1;

            const u2 = await listen('chat-done', () => {
                setLoading(false);
                setPendingActions(null); // Clear any hanging dialogs
                researchTabCreatedRef.current = false; // Reset for next research
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

            // Listen for tool execution completion events (backend emits even without Tool message)
            const u7 = await listen<ToolExecutionCompletedPayload>(EventNames.TOOL_EXECUTION_COMPLETED, (event) => {
                const { tool_call_id, success, tool_name } = event.payload;
                console.log('[TOOL COMPLETED EVENT]', event.payload);
                setMessages((prev): ChatMessage[] =>
                    prev.map((msg): ChatMessage => {
                        if (msg.role === 'Assistant' && msg.tool_calls?.some(tc => tc.id === tool_call_id)) {
                            const updatedCalls = msg.tool_calls.map(tc =>
                                tc.id === tool_call_id
                                    ? {
                                          ...tc,
                                          status: success ? 'complete' : 'error',
                                          result: tc.result ?? (success ? 'Completed' : `Failed: ${tool_name}`),
                                      }
                                    : tc
                            ) as ToolCall[];
                            return { ...msg, tool_calls: updatedCalls };
                        }
                        return msg;
                    })
                );
            });
            unlistenToolCompleted = u7;

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
                if (unlistenToolCompleted) unlistenToolCompleted();
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
            const openFiles = activeFile ? [activeFile] : [];
            const cursorLine = editorState.cursorLine;
            const cursorColumn = editorState.cursorColumn;
            const selectionStartLine = editorState.selectionStartLine;
            const selectionEndLine = editorState.selectionEndLine;
            
            await invoke('send_message', { 
                message: text, 
                modelId: selectedModelId,
                activeFile,
                openFiles,
                cursorLine,
                cursorColumn,
                selectionStartLine,
                selectionEndLine
            });
        } catch (e) {
            console.error(e);
            console.error('Failed to approve tool decision:', e);
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
            await invoke('stop_generation');
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
            await invoke('approve_change', { change_id: changeId });
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
            await invoke('reject_change', { change_id: changeId });
            setPendingChanges(prev => prev.filter(c => c.id !== changeId));
        } catch (e) {
            console.error("Failed to reject change:", e);
        }
    }, []);

    const approveAllChanges = useCallback(async () => {
        try {
            console.log('[useChat] approveAllChanges called; pendingChanges:', pendingChanges.map(c => ({ id: c.id, path: c.path, type: c.change_type })));
            logFrontend(`[approveAllChanges] count=${pendingChanges.length} ids=${pendingChanges.map(c => c.id).join(',')}`);
            await invoke('approve_all_changes');
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
            await invoke('approve_tool', { approved });
            setPendingActions(null);
        } catch (e) {
            console.error('Failed to approve tool:', e);
        }
    }, []);

    const approveToolDecision = useCallback(async (decision: string) => {
        try {
            await invoke('approve_tool_decision', { decision });
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
