import { TodoItem } from './events';

export type ChatMode = 'code' | 'planning';

export interface ChatImage {
    data: string;
    mime_type: string;
    name?: string;
    size?: number;
}

export interface ImageAttachment extends ChatImage {
    id: string;
    dataUrl: string;
    thumbnailUrl: string;
}

export interface ComposerMention {
    kind: 'path';
    path: string;
    is_dir: boolean;
}

export interface QueuedRequest {
    text: string;
    attachments?: ImageAttachment[];
    mentions?: ComposerMention[];
    mode: ChatMode;
}

export interface ChatMessage {
    id?: string;
    role: 'User' | 'Assistant' | 'System' | 'Tool';
    content: string;
    images?: ChatImage[];
    mentions?: ComposerMention[];
    reasoning?: string;
    tool_call_id?: string;
    tool_calls?: ToolCall[];
    progress?: ProgressInfo;
    content_before_tools?: string;
    content_after_tools?: string;
    commandExecutions?: CommandExecution[];
    todos?: TodoItem[];  // Legacy: kept for backward compat with saved conversations
    blocks?: MessageBlock[];
    planSummary?: {
        todos: TodoItem[];
        completedAt: number;
    };
    researchActivities?: ResearchActivity[];
    streaming?: StreamingState;
}

export interface StreamingState {
    seq: number;
    startTime: number;
    lastSeqAt: number;
    activeKind?: 'content' | 'reasoning' | 'tool';
    activeBlockId?: string;
    endTime?: number;
}

export interface ToolActivityState {
    toolName: string;
    filePath: string;
    action: string;
    toolCallId?: string;
    chunkCount: number;
    startedAt: number;
    lastChunkAt: number;
}

export interface CognitiveInterruptState {
    state: 'active' | 'blocked' | 'cleared' | string;
    level?: string;
    frame?: string;
    reason?: string;
    next_action_type?: string;
    summary: string;
    tool_name?: string;
    cleared_by_tool?: string;
    failure_count?: number;
}

export interface HookApprovalRequest {
    sessionId: string;
    approvalId: string;
    toolCallId: string;
    toolName: string;
    arguments: unknown;
    source?: string | null;
    ruleName?: string | null;
    message?: string | null;
    decision?: string | null;
}

export interface ResearchActivity {
    id: string;
    message: string;
    stage: string;
    percent: number;
    timestamp: number;
}

export type MessageBlock =
    | { type: 'text'; content: string; id: string }
    | { type: 'reasoning'; content: string; id: string }
    | { type: 'tool_call'; id: string }
    | { type: 'command_execution'; id: string }  // References commandExecutions by id
    | { type: 'todo'; id: string }  // Legacy: kept for backward compat with saved conversations
    | { type: 'plan_summary'; id: string }  // Compact summary of a completed plan
    | { type: 'research_progress'; id: string };  // References research progress activities

export interface ProgressInfo {
    message: string;
    stage: string;
    percent: number;
}

export interface ToolCall {
    id: string;
    type: string;
    function: {
        name: string;
        arguments: string;
    };
    status?: 'pending' | 'executing' | 'complete' | 'error' | 'skipped';
    result?: string;
}

export interface ModelInfo {
    id: string;
    name: string;
    description: string;
    provider?: string;
    reasoning_effort?: string;
    api_id?: string;
}

export interface CommandExecution {
    id: string;  // Unique ID for referencing in blocks
    command: string;
    cwd?: string;
    output: string;
    exitCode: number;
    duration?: number;
    timestamp: number;
}
