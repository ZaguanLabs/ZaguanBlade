import { TodoItem } from './events';

export type ChatRole = 'User' | 'Assistant' | 'System' | 'Tool';

export interface ChatMessage {
    id?: string;
    role: 'User' | 'Assistant' | 'System' | 'Tool';
    content: string;
    reasoning?: string;
    tool_call_id?: string;
    tool_calls?: ToolCall[];
    progress?: ProgressInfo;
    content_before_tools?: string;
    content_after_tools?: string;
    commandExecutions?: CommandExecution[];
    todos?: TodoItem[];
}

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

export interface ChatState {
    messages: ChatMessage[];
    loading: boolean;
    error: string | null;
}

export interface ModelInfo {
    id: string;
    name: string;
    description: string;
    reasoning_effort?: string;
    api_id?: string;
}

export interface EditProposal {
    id: string;
    path: string;
    old_content: string;
    new_content: string;
    is_new_file?: boolean;
}

export interface CommandExecution {
    command: string;
    cwd?: string;
    output: string;
    exitCode: number;
    duration?: number;
    timestamp: number;
}
