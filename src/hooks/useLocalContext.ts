import { invoke } from '@tauri-apps/api/core';

export interface ConversationIndex {
    id: string;
    project_id: string;
    title: string;
    created_at: string;
    updated_at: string;
    message_count: number;
    tags: string[];
    artifact_path: string;
}

export interface MomentIndex {
    id: string;
    conversation_id: string;
    moment_type: string;
    content: string;
    context: string | null;
    tags: string[];
    created_at: string;
    relevance_score: number;
    artifact_path: string;
}

export interface CodeReferenceIndex {
    id: number;
    conversation_id: string;
    message_id: string;
    file_path: string;
    start_line: number;
    end_line: number;
    context: string | null;
    created_at: string;
}

export interface CodeReference {
    file: string;
    lines: [number, number];
    git_hash?: string;
    context?: string;
    diff?: {
        type: string;
        content: string;
    };
}

export interface Message {
    id: string;
    role: string;
    content: string;
    timestamp: string;
    code_references: CodeReference[];
}

export interface Moment {
    id: string;
    type: string;
    content: string;
    context?: string;
    message_id?: string;
    timestamp: string;
    tags: string[];
    code_references: CodeReference[];
    relevance_score: number;
}

export interface ConversationArtifact {
    version: string;
    conversation_id: string;
    project_id: string;
    created_at: string;
    updated_at: string;
    title: string;
    messages: Message[];
    moments: Moment[];
    metadata: {
        total_messages: number;
        total_tokens: number;
        models_used: string[];
        tags: string[];
    };
}

export async function listLocalConversations(projectPath: string): Promise<ConversationIndex[]> {
    return invoke<ConversationIndex[]>('list_local_conversations', { projectPath });
}

export async function loadLocalConversation(projectPath: string, conversationId: string): Promise<ConversationArtifact> {
    return invoke<ConversationArtifact>('load_local_conversation', { projectPath, conversationId });
}

export async function searchLocalMoments(projectPath: string, query: string, limit: number = 10): Promise<MomentIndex[]> {
    return invoke<MomentIndex[]>('search_local_moments', { projectPath, query, limit });
}

export async function getFileContext(projectPath: string, filePath: string): Promise<CodeReferenceIndex[]> {
    return invoke<CodeReferenceIndex[]>('get_file_context', { projectPath, filePath });
}

export async function deleteLocalConversation(projectPath: string, conversationId: string): Promise<void> {
    return invoke<void>('delete_local_conversation', { projectPath, conversationId });
}

export function useLocalContext(workspacePath: string | null) {
    const listConversations = async (): Promise<ConversationIndex[]> => {
        if (!workspacePath) return [];
        try {
            return await listLocalConversations(workspacePath);
        } catch (e) {
            console.error('[LocalContext] Failed to list conversations:', e);
            return [];
        }
    };

    const loadConversation = async (conversationId: string): Promise<ConversationArtifact | null> => {
        if (!workspacePath) return null;
        try {
            return await loadLocalConversation(workspacePath, conversationId);
        } catch (e) {
            console.error('[LocalContext] Failed to load conversation:', e);
            return null;
        }
    };

    const searchMoments = async (query: string, limit: number = 10): Promise<MomentIndex[]> => {
        if (!workspacePath) return [];
        try {
            return await searchLocalMoments(workspacePath, query, limit);
        } catch (e) {
            console.error('[LocalContext] Failed to search moments:', e);
            return [];
        }
    };

    const getFileRefs = async (filePath: string): Promise<CodeReferenceIndex[]> => {
        if (!workspacePath) return [];
        try {
            return await getFileContext(workspacePath, filePath);
        } catch (e) {
            console.error('[LocalContext] Failed to get file context:', e);
            return [];
        }
    };

    const deleteConversation = async (conversationId: string): Promise<boolean> => {
        if (!workspacePath) return false;
        try {
            await deleteLocalConversation(workspacePath, conversationId);
            return true;
        } catch (e) {
            console.error('[LocalContext] Failed to delete conversation:', e);
            return false;
        }
    };

    return {
        listConversations,
        loadConversation,
        searchMoments,
        getFileRefs,
        deleteConversation,
    };
}
