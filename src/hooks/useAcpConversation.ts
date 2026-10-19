import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AcpAgentStatus, AcpConfigChange, AcpEventPayload, AcpSessionConfig } from '../types/acp';
import { addPrompt, applyAcpEvent, startAgentChat, withConfig, type AgentChat, type AgentTurn } from '../utils/acpConversation';
import { integrationProbeErrorKey } from '../utils/integrationProbe';

interface ConversationOwner {
    id: string;
    execution?: { agent_id: string; agent_name: string } | null;
}

/** A stored conversation an agent ran, which it can reconnect to. */
export interface StoredAgentConversation {
    conversationId: string;
    agentId: string;
    name: string;
}

/** The active conversation's ACP agent, when it has one. A live session is
 * shown from its events; a stored one only names the agent that ran it. */
export function useAcpConversation() {
    const [chat, setChat] = useState<AgentChat | null>(null);
    const [storedOwner, setStoredOwner] = useState<StoredAgentConversation | null>(null);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const conversation = useRef<string | null>(null);

    useEffect(() => {
        const unlisten = listen<AcpEventPayload>('acp-event', ({ payload }) => {
            if (payload.conversationId !== conversation.current) return;
            setChat(current => current && applyAcpEvent(current, payload.event));
        });
        return () => { void unlisten.then(stop => stop()); };
    }, []);

    /** `history`: the stored transcript, shown before the session's own turns. */
    const attach = useCallback((status: AcpAgentStatus, history: AgentTurn[] = []) => {
        conversation.current = status.conversation_id;
        setChat(startAgentChat(status, history));
        setStoredOwner(null);
        setError(null);
        setBusy(false);
    }, []);

    const detach = useCallback(() => {
        conversation.current = null;
        setChat(null);
        setStoredOwner(null);
        setError(null);
        setBusy(false);
    }, []);

    /** Re-reads which agent, if any, owns the active conversation. */
    const sync = useCallback(async () => {
        try {
            const owner = await invoke<ConversationOwner>('get_conversation_metadata');
            if (!owner.execution) { detach(); return; }
            const live = await invoke<AcpAgentStatus | null>('get_acp_conversation', { conversationId: owner.id });
            if (live) {
                if (conversation.current !== live.conversation_id) attach(live);
            } else {
                detach();
                setStoredOwner({ conversationId: owner.id, agentId: owner.execution.agent_id, name: owner.execution.agent_name });
            }
        } catch { detach(); }
    }, [attach, detach]);

    useEffect(() => { void sync(); }, [sync]);

    const run = useCallback(async (operation: (conversationId: string) => Promise<void>) => {
        const id = conversation.current;
        if (!id) return;
        setBusy(true); setError(null);
        try { await operation(id); }
        catch (failure) { if (conversation.current === id) setError(integrationProbeErrorKey(failure)); }
        finally { if (conversation.current === id) setBusy(false); }
    }, []);

    const send = useCallback((text: string) => run(async conversationId => {
        const turn = await invoke<number>('send_acp_prompt', { conversationId, text, images: null });
        if (conversation.current === conversationId) setChat(current => current && addPrompt(current, turn, text));
    }), [run]);

    const cancel = useCallback(() => run(async conversationId => {
        await invoke('cancel_acp_turn', { conversationId });
    }), [run]);

    const answer = useCallback((requestId: string, optionId: string | null) => run(async conversationId => {
        await invoke('respond_acp_permission', { conversationId, requestId, optionId });
    }), [run]);

    const authenticate = useCallback((methodId: string) => run(async conversationId => {
        const status = await invoke<AcpAgentStatus>('authenticate_acp_agent', { conversationId, methodId });
        if (conversation.current === conversationId) setChat(current => current && { ...current, status });
    }), [run]);

    /** Applies one of the agent's own settings; the agent returns the full new state. */
    const configure = useCallback((change: AcpConfigChange) => run(async conversationId => {
        const config = await invoke<AcpSessionConfig>('set_acp_config', { conversationId, change });
        if (conversation.current === conversationId) setChat(current => current && withConfig(current, config));
    }), [run]);

    /** Ends the session; the transcript stays on screen. */
    const end = useCallback(() => run(async conversationId => {
        await invoke('close_acp_conversation', { conversationId });
        if (conversation.current === conversationId) {
            setChat(current => current && applyAcpEvent({
                ...current,
                turns: current.turns.map(turn => turn.state === 'running' ? { ...turn, state: 'failed', error: 'cancelled' } : turn),
            }, { type: 'exited', error: null }));
        }
    }), [run]);

    return { chat, storedOwner, busy, error, attach, detach, sync, send, cancel, answer, authenticate, configure, end };
}
