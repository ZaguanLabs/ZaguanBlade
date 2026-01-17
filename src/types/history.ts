export interface ConversationSummary {
    id: string;
    project_id: string;
    title: string;
    created_at: string;
    last_active_at: string;
    message_count: number;
    preview: string;
}

export interface ConversationListResponse {
    conversations: ConversationSummary[];
}

export interface HistoryMessage {
    role: 'user' | 'assistant' | 'tool' | 'system';
    content: string;
    tool_calls?: Array<{
        id: string;
        type: string;
        function: {
            name: string;
            arguments: string;
        };
    }>;
    tool_call_id?: string;
    created_at: string;
}

export interface FullConversation {
    session_id: string;
    project_id: string;
    title: string;
    created_at: string;
    last_active_at: string;
    message_count: number;
    messages: HistoryMessage[];
}
