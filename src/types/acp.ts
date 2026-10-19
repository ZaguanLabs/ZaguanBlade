import type { AcpForwardingView } from './integrations';
import type { ElicitationRequest } from '../utils/mcpElicitation';
/** Mirrors `integrations::acp_session` and `agent_sessions` (snake_case on the wire). */

export interface AcpAuthMethod {
    id: string;
    name: string;
    description: string | null;
    /** Signs in through the agent's own program in a terminal Blade runs. */
    terminal: boolean;
}

export interface AcpAgentSnapshot {
    protocol_version: number;
    agent: { name: string; title: string | null; version: string } | null;
    load_session: boolean;
    resume_session: boolean;
    close_session: boolean;
    list_sessions: boolean;
    logout: boolean;
    prompt_image: boolean;
    prompt_audio: boolean;
    prompt_embedded_context: boolean;
    mcp_http: boolean;
    mcp_sse: boolean;
    auth_methods: AcpAuthMethod[];
}

export interface AcpConfigChoice {
    value: string;
    name: string;
    description: string | null;
}

/** One of the agent's session settings, in the agent's order. */
export type AcpConfigOption = {
    id: string;
    name: string;
    description: string | null;
    /** `mode`, `model`, `model_config`, `thought_level` or the agent's own. */
    category: string | null;
} & (
    | { type: 'select'; current_value: string; groups: Array<{ name: string | null; choices: AcpConfigChoice[] }> }
    | { type: 'boolean'; current_value: boolean }
);

export interface AcpSessionConfig {
    options: AcpConfigOption[];
    /** Legacy session modes, only when the agent offers no options. */
    modes: { current_mode_id: string; modes: AcpConfigChoice[] } | null;
}

export type AcpConfigChange =
    | { type: 'select'; config_id: string; value: string }
    | { type: 'boolean'; config_id: string; value: boolean }
    | { type: 'mode'; mode_id: string };

export interface AcpAgentStatus {
    conversation_id: string;
    integration_id: string;
    name: string;
    snapshot: AcpAgentSnapshot;
    /** False until the agent has created its session (it may need a sign-in). */
    ready: boolean;
    forwarding?: AcpForwardingView;
    config?: AcpSessionConfig;
    /** How the session began; null until it exists. */
    reopened?: AcpReopened | null;
}

/** `resumed`/`loaded`: the agent continued the conversation's earlier
 * session. `replaced`: it could not, and started afresh. */
export type AcpReopened = 'new' | 'resumed' | 'loaded' | 'replaced';

export type AcpContent =
    | { type: 'text'; text: string }
    | { type: 'link'; uri: string; name: string }
    | { type: 'resource'; uri: string; text: string | null }
    | { type: 'image'; mime_type: string }
    | { type: 'audio' };

export type AcpToolKind = 'read' | 'edit' | 'delete' | 'move' | 'search' | 'execute' | 'think' | 'fetch' | 'switch_mode' | 'other';
export type AcpToolStatus = 'pending' | 'in_progress' | 'completed' | 'failed';

export type AcpToolContent =
    | { type: 'content'; content: AcpContent }
    | { type: 'diff'; path: string; old_text: string | null; new_text: string }
    | { type: 'terminal'; terminal_id: string };

export interface AcpToolCall {
    id: string;
    title: string;
    kind: AcpToolKind;
    status: AcpToolStatus;
    locations: Array<{ path: string; line: number | null }>;
    content: AcpToolContent[];
    truncated: boolean;
}

export interface AcpPlanEntry {
    content: string;
    priority: 'high' | 'medium' | 'low';
    status: 'pending' | 'in_progress' | 'completed';
}

export type AcpPermissionKind = 'allow_once' | 'allow_always' | 'reject_once' | 'reject_always';

export interface AcpPermission {
    request_id: string;
    tool_call: AcpToolCall;
    options: Array<{ id: string; name: string; kind: AcpPermissionKind }>;
}

/** The agent's request for input. Agent text is shown as text. */
export type AcpElicitation = ElicitationRequest & { tool_call_id: string | null };

/** A command the agent runs in a terminal Blade owns. `output` is the latest
 * part only; a released terminal keeps its last output. */
export interface AcpTerminal {
    id: string;
    command: string;
    cwd: string;
    output: string;
    truncated: boolean;
    exit: { exit_code: number | null; signal: string | null } | null;
    released: boolean;
}

/** Why Blade did not let the agent write a file (others reach only the agent). */
export type AcpFileRefusal = 'unsaved_changes' | 'protected' | 'outside_workspace';

export type AcpStopReason = 'end_turn' | 'max_tokens' | 'max_turn_requests' | 'refusal' | 'cancelled';

/** What an agent reported during a turn, kept with the transcript. */
export interface AcpAgentActivity {
    agent_name: string;
    tool_calls?: AcpToolCall[];
    /** Reports beyond the stored limit. */
    omitted_tool_calls?: number;
    plan?: AcpPlanEntry[];
    stop_reason?: AcpStopReason | null;
    /** A runtime error code when the turn did not end normally. */
    error?: string | null;
    text_truncated?: boolean;
}

export type AcpEvent =
    | { type: 'message_chunk'; turn: number | null; content: AcpContent }
    | { type: 'thought_chunk'; turn: number | null; content: AcpContent }
    | { type: 'text_truncated'; turn: number | null }
    | { type: 'tool_call'; turn: number | null; call: AcpToolCall }
    | { type: 'plan'; turn: number | null; entries: AcpPlanEntry[] }
    | { type: 'permission'; turn: number | null; request: AcpPermission }
    | { type: 'permission_resolved'; request_id: string; option_id: string | null }
    | { type: 'elicitation'; turn: number | null; request: AcpElicitation }
    | { type: 'elicitation_resolved'; request_id: string; answer: 'accept' | 'decline' | 'cancel' }
    | { type: 'config_changed'; config: AcpSessionConfig }
    | { type: 'title_changed'; title: string | null }
    | { type: 'file_written'; turn: number | null; path: string }
    | { type: 'terminal'; turn: number | null; terminal: AcpTerminal }
    | { type: 'file_refused'; turn: number | null; path: string; reason: AcpFileRefusal }
    | { type: 'usage'; used: number; size: number }
    | { type: 'turn_ended'; turn: number; stop_reason: AcpStopReason }
    | { type: 'turn_failed'; turn: number; error: string }
    | { type: 'exited'; error: string | null };

export interface AcpEventPayload {
    conversationId: string;
    event: AcpEvent;
}
