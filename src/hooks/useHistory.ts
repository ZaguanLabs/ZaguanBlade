import { useState, useCallback, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import { subscribeBladeNestedEventType, waitForBladeEvent } from '../services/bladeEvents';
import type { ConversationSummary, HistoryMessage } from '../types/blade';
import type { ChatMessage } from '../types/chat';
import { ensureMessagesHaveBlocks } from '../utils/messageBlocks';
import { formatUnknownBackendError } from '../utils/backendErrors';

function mapHistoryRole(role: string): ChatMessage['role'] {
    if (role === 'User' || role === 'user') return 'User';
    if (role === 'Assistant' || role === 'assistant') return 'Assistant';
    if (role === 'Tool' || role === 'tool') return 'Tool';
    return 'System';
}

function mapHistoryMessage(msg: HistoryMessage | any): ChatMessage {
    const commandExecutions = (msg.commandExecutions || msg.command_executions)?.map((execution: any) => ({
        id: execution.id,
        command: execution.command,
        cwd: execution.cwd,
        output: execution.output,
        exitCode: execution.exitCode ?? execution.exit_code ?? 0,
        duration: execution.duration,
        timestamp: execution.timestamp,
    }));

    return {
        id: crypto.randomUUID(),
        role: mapHistoryRole(msg.role),
        content: msg.content,
        reasoning: msg.reasoning,
        tool_call_id: msg.tool_call_id,
        tool_calls: msg.tool_calls,
        content_before_tools: msg.content_before_tools,
        content_after_tools: msg.content_after_tools,
        commandExecutions,
    };
}

export function useHistory() {
    const [conversations, setConversations] = useState<ConversationSummary[]>([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    // Listen for History Events from backend
    useEffect(() => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
            console.warn('[useHistory] Not in Tauri environment, skipping listener setup');
            return;
        }

        console.debug('[useHistory] Setting up blade-event listener');
        const unsubscribeConversationList = subscribeBladeNestedEventType(
            'History',
            'ConversationList',
            (payload) => {
                console.debug('[useHistory] ConversationList received with', payload.conversations.length, 'conversations');
                if (payload.conversations.length > 0) {
                    const sample = payload.conversations[0];
                    void invoke('log_frontend', {
                        message: `[useHistory] Sample conversation: id=${sample.id}, created_at=${sample.created_at}, last_active_at=${sample.last_active_at}`
                    });
                }
                setConversations(payload.conversations);
                setLoading(false);
            }
        );
        const unsubscribeConversationLoaded = subscribeBladeNestedEventType(
            'History',
            'ConversationLoaded',
            () => {
                setLoading(false);
            },
        );
        console.debug('[useHistory] blade-event listener set up successfully');

        return () => {
            console.debug('[useHistory] Cleaning up blade-event listener');
            unsubscribeConversationList();
            unsubscribeConversationLoaded();
        };
    }, []);

    const fetchConversations = useCallback(async (projectId: string) => {
        try {
            console.debug('[useHistory] fetchConversations called with projectId:', projectId);
            setLoading(true);
            setError(null);

            // Check storage mode settings
            let useLocal = false;
            try {
                const isLocalActive = await invoke<boolean>('is_local_model_active').catch(() => false);
                if (isLocalActive) {
                    useLocal = true;
                } else {
                    const settings = await invoke<any>('load_project_settings', { projectPath: projectId });
                    if (settings?.storage?.mode === 'local') {
                        useLocal = true;
                    }
                }
            } catch (e) {
                console.warn('[useHistory] Failed to load settings, defaulting to server mode', e);
            }

            if (useLocal) {
                console.debug('[useHistory] Loading conversations from LOCAL storage');
                const localConversations = await invoke<any[]>('list_conversations');

                // Map Rust metadata to UI Summary format
                const conversations: ConversationSummary[] = localConversations.map(c => ({
                    id: c.id,
                    project_id: projectId,
                    title: c.title,
                    created_at: c.created_at,
                    last_active_at: c.updated_at, // Map updated_at to last_active_at
                    message_count: c.message_count,
                    preview: '', // Local metadata might not have preview yet
                }));

                setConversations(conversations);
                setLoading(false);
            } else {
                // SERVER mode
                // Dispatch ListConversations Intent via BCP
                console.debug('[useHistory] Dispatching ListConversations intent (SERVER)...');
                await BladeDispatcher.history({
                    type: 'ListConversations',
                    payload: { project_id: projectId }
                });
                console.debug('[useHistory] ListConversations intent dispatched successfully');
                // Backend will respond with ConversationList Event
            }

        } catch (e) {
            console.error('[useHistory] Failed to fetch conversation history:', e);
            setError(formatUnknownBackendError(e));
            setLoading(false);
        }
    }, []);

    const loadConversation = useCallback(async (sessionId: string): Promise<ChatMessage[]> => {
        // We need projectId to check settings, but loadConversation doesn't take it as arg.
        // However, we can assume if we are loading a conversation, we might need to check Global or try local first.
        // Or better: try local load, if it fails then server?
        // Actually, if we are in local mode, we should ONLY try local.
        // Since we don't have projectId here easily (it's in the component), let's try to detect mode via
        // 'get_current_workspace' or similar?
        // For now, let's just try local load first if we can, or check global settings?
        // A safer bet: The ID itself might tell us? No.
        // Let's assume we can check the current workspace.

        let useLocal = false;
        try {
            const isLocalActive = await invoke<boolean>('is_local_model_active').catch(() => false);
            if (isLocalActive) {
                useLocal = true;
            } else {
                const currentPath = await invoke<string | null>('get_current_workspace');
                if (currentPath) {
                    const settings = await invoke<any>('load_project_settings', { projectPath: currentPath });
                    if (settings?.storage?.mode === 'local') {
                        useLocal = true;
                    }
                }
            }
        } catch (e) {
            console.warn('Failed to check usage mode in loadConversation', e);
        }

        if (useLocal) {
            console.debug('[useHistory] Loading conversation from LOCAL storage');
            setLoading(true);
            setError(null);
            try {
                await invoke('load_conversation', { id: sessionId });
                const messages = await invoke<any[]>('get_conversation');

                const chatMessages: ChatMessage[] = messages.map(mapHistoryMessage);

                setLoading(false);
                return ensureMessagesHaveBlocks(chatMessages);
            } catch (e) {
                console.error('Failed local load:', e);
                setError(formatUnknownBackendError(e));
                setLoading(false);
                throw e;
            }
        } else {
            return new Promise((resolve, reject) => {
                setLoading(true);
                setError(null);

                const loadFromServer = async () => {
                    try {
                        const eventPromise = waitForBladeEvent((envelope) => {
                            if (envelope.event.type !== 'History') {
                                return false;
                            }

                            const historyEvent = envelope.event.payload;
                            return historyEvent.type === 'ConversationLoaded'
                                && historyEvent.payload.session_id === sessionId;
                        }, 15000);

                        await BladeDispatcher.history({
                            type: 'LoadConversation',
                            payload: { session_id: sessionId }
                        });

                        const event = await eventPromise;
                        const historyEvent = event.event.payload as {
                            type: 'ConversationLoaded';
                            payload: {
                                session_id: string;
                                messages: HistoryMessage[];
                            };
                        };
                        const messages: ChatMessage[] = historyEvent.payload.messages.map((msg: HistoryMessage) => ({
                            ...mapHistoryMessage(msg),
                            tool_calls: msg.tool_calls?.map((tc) => ({
                                ...tc,
                                status: 'complete' as const
                            })),
                        }));

                        setLoading(false);
                        resolve(ensureMessagesHaveBlocks(messages));
                    } catch (e) {
                        console.error('Failed to load conversation:', e);
                        setError(formatUnknownBackendError(e));
                        setLoading(false);
                        reject(e);
                    }
                };

                void loadFromServer();
            });
        }
    }, []);

    return {
        conversations,
        loading,
        error,
        fetchConversations,
        loadConversation
    };
}
