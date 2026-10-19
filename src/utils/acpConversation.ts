import type { ChatImage } from '../types/chat';
import type { AcpAgentActivity, AcpAgentStatus, AcpEvent, AcpFileRefusal, AcpPermission, AcpPlanEntry, AcpSessionConfig, AcpStopReason, AcpTerminal, AcpToolCall } from '../types/acp';
import type { IntegrationLaunchReview } from '../types/integrations';

/** One prompt and what the agent reported for it. Tool calls are reports of
 * the agent's own activity; Blade never runs them. `files` are the writes the
 * agent made through Blade, which the user reviews in the editor. */
export interface AgentTurn {
    turn: number;
    prompt: string;
    /** Images sent with the prompt. */
    images?: ChatImage[];
    text: string;
    thoughts: string;
    tools: AcpToolCall[];
    plan: AcpPlanEntry[];
    files: Array<{ path: string; refused: AcpFileRefusal | null }>;
    /** Terminals the agent started during this turn. */
    terminals: string[];
    state: 'running' | 'ended' | 'failed';
    stopReason: AcpStopReason | null;
    error: string | null;
    truncated: boolean;
    /** From the saved transcript, before this session; live events never change it. */
    stored?: boolean;
    /** Tool reports the transcript did not keep. */
    omittedTools?: number;
}

interface StoredMessage {
    role: string;
    content: string;
    reasoning?: string;
    images?: ChatImage[];
    agent_activity?: AcpAgentActivity;
}

function storedReply(message: StoredMessage): Partial<AgentTurn> {
    const activity = message.agent_activity;
    return {
        text: message.content,
        thoughts: message.reasoning ?? '',
        tools: activity?.tool_calls ?? [],
        omittedTools: activity?.omitted_tool_calls ?? 0,
        plan: activity?.plan ?? [],
        state: activity?.error ? 'failed' : 'ended',
        stopReason: activity?.stop_reason ?? null,
        error: activity?.error ?? null,
        truncated: activity?.text_truncated ?? false,
    };
}

export interface AgentChat {
    status: AcpAgentStatus;
    turns: AgentTurn[];
    /** Requests waiting for the user, in arrival order. */
    permissions: AcpPermission[];
    exited: { error: string | null } | null;
    title: string | null;
    usage: { used: number; size: number } | null;
    /** Every terminal the agent ran through Blade, by id. */
    terminals: Record<string, AcpTerminal>;
}

export function startAgentChat(status: AcpAgentStatus, history: AgentTurn[] = []): AgentChat {
    return { status, turns: history, permissions: [], exited: null, title: null, usage: null, terminals: {} };
}

/** The saved transcript as finished turns. Their numbers are negative, so
 * they never meet the session's own turns, which count from 1. */
export function storedTurns(messages: ReadonlyArray<StoredMessage>): AgentTurn[] {
    const turns: AgentTurn[] = [];
    // A prompt still waiting for its reply.
    let open: AgentTurn | null = null;
    for (const message of messages) {
        if (message.role === 'User') {
            open = { ...emptyTurn(0, message.content), images: message.images, state: 'ended', stored: true };
            turns.push(open);
        } else if (message.role === 'Assistant') {
            if (open) Object.assign(open, storedReply(message));
            else turns.push({ ...emptyTurn(0), ...storedReply(message), stored: true });
            open = null;
        }
    }
    return turns.map((turn, index) => ({ ...turn, turn: index - turns.length }));
}

function emptyTurn(turn: number, prompt = ''): AgentTurn {
    return { turn, prompt, text: '', thoughts: '', tools: [], plan: [], files: [], terminals: [], state: 'running', stopReason: null, error: null, truncated: false };
}

/** Updates turn `turn` (or the latest one when `turn` is null), creating it
 * when events arrive before the prompt call returned. */
function withTurn(chat: AgentChat, turn: number | null, change: (current: AgentTurn) => AgentTurn): AgentChat {
    const index = turn === null ? chat.turns.length - 1 : chat.turns.findIndex(entry => entry.turn === turn);
    if (index >= 0 && !chat.turns[index].stored) {
        const turns = chat.turns.slice();
        turns[index] = change(turns[index]);
        return { ...chat, turns };
    }
    if (turn === null || index >= 0) return chat;
    return { ...chat, turns: [...chat.turns, change(emptyTurn(turn))] };
}

export function addPrompt(chat: AgentChat, turn: number, prompt: string, images?: ChatImage[]): AgentChat {
    return withTurn(chat, turn, current => ({ ...current, prompt, images: images?.length ? images : undefined }));
}

/** Takes the agent's full settings state. */
export function withConfig(chat: AgentChat, config: AcpSessionConfig): AgentChat {
    return { ...chat, status: { ...chat.status, config } };
}

export function runningTurn(chat: AgentChat): AgentTurn | undefined {
    return chat.turns.find(turn => turn.state === 'running');
}

export function applyAcpEvent(chat: AgentChat, event: AcpEvent): AgentChat {
    switch (event.type) {
        case 'message_chunk':
        case 'thought_chunk': {
            if (event.content.type !== 'text') return chat;
            const text = event.content.text;
            return withTurn(chat, event.turn, current => event.type === 'message_chunk'
                ? { ...current, text: current.text + text }
                : { ...current, thoughts: current.thoughts + text });
        }
        case 'text_truncated':
            return withTurn(chat, event.turn, current => ({ ...current, truncated: true }));
        case 'tool_call':
            return withTurn(chat, event.turn, current => {
                const known = current.tools.findIndex(call => call.id === event.call.id);
                const tools = current.tools.slice();
                if (known >= 0) tools[known] = event.call; else tools.push(event.call);
                return { ...current, tools };
            });
        case 'plan':
            return withTurn(chat, event.turn, current => ({ ...current, plan: event.entries }));
        case 'terminal': {
            const { id } = event.terminal;
            const known = { ...chat, terminals: { ...chat.terminals, [id]: event.terminal } };
            return withTurn(known, event.turn, current => current.terminals.includes(id)
                ? current
                : { ...current, terminals: [...current.terminals, id] });
        }
        case 'file_written':
        case 'file_refused': {
            const file = { path: event.path, refused: event.type === 'file_refused' ? event.reason : null };
            return withTurn(chat, event.turn, current => ({
                ...current,
                files: [...current.files.filter(known => known.path !== file.path), file],
            }));
        }
        case 'permission':
            return chat.permissions.some(request => request.request_id === event.request.request_id)
                ? chat
                : { ...chat, permissions: [...chat.permissions, event.request] };
        case 'permission_resolved':
            return { ...chat, permissions: chat.permissions.filter(request => request.request_id !== event.request_id) };
        case 'config_changed':
            return withConfig(chat, event.config);
        case 'title_changed':
            return { ...chat, title: event.title };
        case 'usage':
            return { ...chat, usage: { used: event.used, size: event.size } };
        case 'turn_ended':
            return withTurn(chat, event.turn, current => ({ ...current, state: 'ended', stopReason: event.stop_reason }));
        case 'turn_failed':
            return withTurn(chat, event.turn, current => ({ ...current, state: 'failed', error: event.error }));
        case 'exited':
            return {
                ...chat,
                exited: { error: event.error },
                permissions: [],
                turns: chat.turns.map(turn => turn.state === 'running' ? { ...turn, state: 'failed', error: turn.error ?? 'agent_exited' } : turn),
            };
        default:
            return chat;
    }
}

type Invoke = <T>(command: string, args: Record<string, unknown>) => Promise<T>;

/** Owns the pending launch approval for an agent conversation. Once started,
 * the session belongs to the backend and ends when the conversation changes. */
export class AcpStartRequest {
    private disposed = false;
    private ticket: string | null = null;
    private running = false;
    constructor(private readonly invoke: Invoke) {}

    async prepare(integrationId: string, expectedRevision: string, workspacePath: string): Promise<IntegrationLaunchReview | null> {
        const review = await this.invoke<IntegrationLaunchReview>('prepare_acp_conversation', { integrationId, expectedRevision, workspacePath });
        if (this.disposed) { await this.cancel(review.ticket_id); return null; }
        this.ticket = review.ticket_id;
        return review;
    }

    /** With `conversationId`, the agent reconnects to that stored conversation. */
    async start(conversationId?: string): Promise<AcpAgentStatus | null> {
        if (this.disposed || !this.ticket || this.running) return null;
        this.running = true;
        const ticketId = this.ticket;
        try {
            return await this.invoke<AcpAgentStatus>('start_acp_conversation', conversationId ? { ticketId, conversationId } : { ticketId });
        } finally { this.ticket = null; }
    }

    dispose(): void {
        this.disposed = true;
        if (this.ticket) { void this.cancel(this.ticket); this.ticket = null; }
    }

    private async cancel(ticketId: string): Promise<void> {
        try { await this.invoke('cancel_integration_test', { ticketId }); } catch { /* the approval expires on its own */ }
    }
}
