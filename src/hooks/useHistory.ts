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

    const fetchConversations = useCallback(async (projectId: string) => {
        try {
            console.log('[useHistory] fetchConversations called with projectId:', projectId);
            setLoading(true);
            setError(null);

            // Check storage mode settings
            let useLocal = false;
            try {
                const settings = await invoke<any>('load_project_settings', { projectPath: projectId });
                if (settings?.storage?.mode === 'local') {
                    useLocal = true;
                }
            } catch (e) {
                console.warn('[useHistory] Failed to load settings, defaulting to server mode', e);
            }

            if (useLocal) {
                console.log('[useHistory] Loading conversations from LOCAL storage');
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
                console.log('[useHistory] Dispatching ListConversations intent (SERVER)...');
                await BladeDispatcher.history({
                    type: 'ListConversations',
                    payload: { project_id: projectId }
                });
                console.log('[useHistory] ListConversations intent dispatched successfully');
                // Backend will respond with ConversationList Event
            }

        } catch (e) {
            console.error('[useHistory] Failed to fetch conversation history:', e);
            setError(e instanceof Error ? e.message : String(e));
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
            const currentPath = await invoke<string | null>('get_current_workspace');
            if (currentPath) {
                const settings = await invoke<any>('load_project_settings', { projectPath: currentPath });
                if (settings?.storage?.mode === 'local') {
                    useLocal = true;
                }
            }
        } catch (e) {
            console.warn('Failed to check usage mode in loadConversation', e);
        }

        if (useLocal) {
            console.log('[useHistory] Loading conversation from LOCAL storage');
            setLoading(true);
            setError(null);
            try {
                await invoke('load_conversation', { id: sessionId });
                const messages = await invoke<any[]>('get_conversation');

                // Map to ChatMessage
                const chatMessages: ChatMessage[] = messages.map(msg => ({
                    id: crypto.randomUUID(), // Local messages might not have UUIDs stored in message struct if not migrated
                    role: msg.role === 'User' ? 'User' : msg.role === 'Assistant' ? 'Assistant' : msg.role === 'Tool' ? 'Tool' : 'System', // Rust types might be different
                    content: msg.content,
                    reasoning: msg.reasoning,
                    tool_call_id: msg.tool_call_id,
                    tool_calls: msg.tool_calls // Ensure this field exists or is mapped
                }));

                setLoading(false);
                return ensureMessagesHaveBlocks(chatMessages);
            } catch (e) {
                console.error('Failed local load:', e);
                setError(String(e));
                setLoading(false);
                throw e;
            }
        } else {
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
                            payload: { session_id: sessionId }
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
