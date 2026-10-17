/** Mirrors `integrations::acp_session` and `agent_sessions` (snake_case on the wire). */

export interface AcpAuthMethod {
    id: string;
    name: string;
    description: string | null;
    /** Needs an interactive terminal login, which Blade does not offer yet. */
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

export interface AcpAgentStatus {
    conversation_id: string;
    integration_id: string;
    name: string;
    snapshot: AcpAgentSnapshot;
    /** False until the agent has created its session (it may need a sign-in). */
    ready: boolean;
}

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

export type AcpStopReason = 'end_turn' | 'max_tokens' | 'max_turn_requests' | 'refusal' | 'cancelled';

export type AcpEvent =
    | { type: 'message_chunk'; turn: number | null; content: AcpContent }
    | { type: 'thought_chunk'; turn: number | null; content: AcpContent }
    | { type: 'text_truncated'; turn: number | null }
    | { type: 'tool_call'; turn: number | null; call: AcpToolCall }
    | { type: 'plan'; turn: number | null; entries: AcpPlanEntry[] }
    | { type: 'permission'; turn: number | null; request: AcpPermission }
    | { type: 'permission_resolved'; request_id: string; option_id: string | null }
    | { type: 'mode_changed'; mode_id: string }
    | { type: 'title_changed'; title: string | null }
    | { type: 'usage'; used: number; size: number }
    | { type: 'turn_ended'; turn: number; stop_reason: AcpStopReason }
    | { type: 'turn_failed'; turn: number; error: string }
    | { type: 'exited'; error: string | null };

export interface AcpEventPayload {
    conversationId: string;
    event: AcpEvent;
}
