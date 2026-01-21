import { useState, useCallback, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { BladeDispatcher } from '../services/blade';
import type { ConversationSummary, BladeEventEnvelope } from '../types/blade';
import type { ChatMessage } from '../types/chat';
import { ensureMessagesHaveBlocks } from '../utils/messageBlocks';

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

        console.log('[useHistory] Setting up blade-event listener');
        let unlistenHistory: (() => void) | undefined;

        const setupListener = async () => {
            const unlisten = await listen<BladeEventEnvelope>('blade-event', (event) => {
                const envelope = event.payload;
                console.log('[useHistory] Received blade-event:', envelope.event.type);

                if (envelope.event.type === 'History') {
                    const historyEvent = envelope.event.payload;
                    console.log('[useHistory] History event type:', historyEvent.type);

                    if (historyEvent.type === 'ConversationList') {
                        console.log('[useHistory] ConversationList received with', historyEvent.payload.conversations.length, 'conversations');
                        if (historyEvent.payload.conversations.length > 0) {
                            const sample = historyEvent.payload.conversations[0];
                            // Use Tauri's invoke to log to terminal
                            invoke('log_frontend', {
                                message: `[useHistory] Sample conversation: id=${sample.id}, created_at=${sample.created_at}, last_active_at=${sample.last_active_at}`
                            });
                        }
                        setConversations(historyEvent.payload.conversations);
                        setLoading(false);
                    } else if (historyEvent.type === 'ConversationLoaded') {
                        // This will be handled by the callback promise resolution
                        setLoading(false);
                    }
                }
            });
            console.log('[useHistory] blade-event listener set up successfully');
            unlistenHistory = unlisten;
        };

        setupListener();

        return () => {
            console.log('[useHistory] Cleaning up blade-event listener');
            if (unlistenHistory) unlistenHistory();
        };
    }, []);

    const fetchConversations = useCallback(async (userId: string, projectId: string) => {
        try {
            console.log('[useHistory] fetchConversations called with userId:', userId, 'projectId:', projectId);
            setLoading(true);
            setError(null);

            // Dispatch ListConversations Intent via BCP
            console.log('[useHistory] Dispatching ListConversations intent...');
            await BladeDispatcher.history({
                type: 'ListConversations',
                payload: { user_id: userId, project_id: projectId }
            });
            console.log('[useHistory] ListConversations intent dispatched successfully');

            // Backend will respond with ConversationList Event
        } catch (e) {
            console.error('[useHistory] Failed to fetch conversation history:', e);
            setError(e instanceof Error ? e.message : String(e));
            setLoading(false);
        }
    }, []);

    const loadConversation = useCallback(async (sessionId: string, userId: string): Promise<ChatMessage[]> => {
        return new Promise((resolve, reject) => {
            setLoading(true);
            setError(null);

            // Set up one-time listener for the ConversationLoaded event
            const setupOneTimeListener = async () => {
                const unlisten = await listen<BladeEventEnvelope>('blade-event', (event) => {
                    const envelope = event.payload;

                    if (envelope.event.type === 'History') {
                        const historyEvent = envelope.event.payload;

                        if (historyEvent.type === 'ConversationLoaded' &&
                            historyEvent.payload.session_id === sessionId) {

                            // Convert history messages to ChatMessage format
                            const messages: ChatMessage[] = historyEvent.payload.messages.map(msg => ({
                                id: crypto.randomUUID(),
                                role: msg.role === 'user' ? 'User' :
                                    msg.role === 'assistant' ? 'Assistant' :
                                        msg.role === 'tool' ? 'Tool' : 'System',
                                content: msg.content,
                                // Mark all tool calls as complete since they're historical
                                tool_calls: msg.tool_calls?.map(tc => ({
                                    ...tc,
                                    status: 'complete' as const
                                })),
                                tool_call_id: msg.tool_call_id
                            }));

                            unlisten();
                            setLoading(false);
                            // Reconstruct blocks for proper conversation flow ordering
                            resolve(ensureMessagesHaveBlocks(messages));
                        }
                    }
                });

                // Dispatch LoadConversation Intent via BCP
                try {
                    await BladeDispatcher.history({
                        type: 'LoadConversation',
                        payload: { session_id: sessionId, user_id: userId }
                    });
                } catch (e) {
                    console.error('Failed to load conversation:', e);
                    setError(e instanceof Error ? e.message : String(e));
                    setLoading(false);
                    unlisten();
                    reject(e);
                }
            };

            setupOneTimeListener();
        });
    }, []);

    return {
        conversations,
        loading,
        error,
        fetchConversations,
        loadConversation
    };
}
