import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AcpAgentStatus, AcpConfigChange, AcpEventPayload, AcpSessionConfig } from '../types/acp';
import type { ChatImage } from '../types/chat';
import { addPrompt, applyAcpEvent, startAgentChat, withConfig, type AgentChat, type AgentTurn } from '../utils/acpConversation';
import { integrationProbeErrorKey } from '../utils/integrationProbe';
import type { ElicitationAnswer } from '../components/McpElicitation';

/** How long the sign-in terminal has to appear before the sign-in is given up. */
const TERMINAL_READY_TIMEOUT_MS = 10_000;

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

/** A terminal sign-in shown in the agent view. `done`: it failed and its
 * terminal stays open so the user can read what the program printed. */
export interface TerminalSignIn {
    terminalId: string;
    method: string;
    done: boolean;
}

/** The active conversation's ACP agent, when it has one. A live session is
 * shown from its events; a stored one only names the agent that ran it. */
export function useAcpConversation() {
    const [chat, setChat] = useState<AgentChat | null>(null);
    const [storedOwner, setStoredOwner] = useState<StoredAgentConversation | null>(null);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [signIn, setSignIn] = useState<TerminalSignIn | null>(null);
    const conversation = useRef<string | null>(null);
    /** The sign-in terminal the user closed; its failure is not an error. */
    const closedSignIn = useRef<string | null>(null);

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
        setSignIn(null);
    }, []);

    const detach = useCallback(() => {
        conversation.current = null;
        setChat(null);
        setStoredOwner(null);
        setError(null);
        setBusy(false);
        setSignIn(null);
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

    const send = useCallback((text: string, images: ChatImage[] = []) => run(async conversationId => {
        const turn = await invoke<number>('send_acp_prompt', { conversationId, text, images: images.length ? images : null });
        if (conversation.current === conversationId) setChat(current => current && addPrompt(current, turn, text, images));
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

    const answerElicitation = useCallback((requestId: string, answer: ElicitationAnswer, content?: Record<string, unknown>) => run(async conversationId => {
        await invoke('respond_acp_elicitation', { conversationId, requestId, answer, content: content ?? null });
    }), [run]);

    /** Runs the agent's own program in a terminal shown in the view, once that
     * terminal is ready. When it signs in, the agent is started again in the
     * same conversation and its new status replaces the old one. */
    const signInInTerminal = useCallback(async (methodId: string, method: string) => {
        const conversationId = conversation.current;
        if (!conversationId) return;
        const terminalId = `acp-sign-in-${crypto.randomUUID()}`;
        let ready!: () => void;
        const shown = new Promise<void>(resolve => { ready = resolve; });
        const stop = await listen<{ id: string }>('terminal-ready', ({ payload }) => { if (payload.id === terminalId) ready(); });
        let timer: ReturnType<typeof setTimeout> | undefined;
        setBusy(true); setError(null);
        setSignIn({ terminalId, method, done: false });
        try {
            await Promise.race([shown, new Promise<never>((_, reject) => {
                timer = setTimeout(() => reject('launch_failed'), TERMINAL_READY_TIMEOUT_MS);
            })]);
            const status = await invoke<AcpAgentStatus>('sign_in_acp_agent_in_terminal', { conversationId, methodId, terminalId });
            if (conversation.current === conversationId) {
                setChat(current => current && { ...current, status });
                setSignIn(null);
            }
        } catch (failure) {
            if (conversation.current === conversationId && closedSignIn.current !== terminalId) {
                setError(failure === 'authorization_failed' ? 'chat.agent.signIn.terminalFailed' : integrationProbeErrorKey(failure));
                setSignIn(current => current?.terminalId === terminalId ? { ...current, done: true } : current);
            }
        } finally {
            clearTimeout(timer);
            stop();
            if (conversation.current === conversationId) setBusy(false);
        }
    }, []);

    /** Closes the sign-in terminal, which stops its program if it still runs. */
    const closeSignIn = useCallback(() => {
        setSignIn(current => {
            if (current) closedSignIn.current = current.terminalId;
            return null;
        });
    }, []);

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

    return {
        chat, storedOwner, busy, error, signIn, attach, detach, sync, send, cancel, answer, answerElicitation,
        authenticate, signInInTerminal, closeSignIn, configure, end,
    };
}
