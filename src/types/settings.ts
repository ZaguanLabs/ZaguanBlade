export interface RemoteAiConfig {
    api_key: string;
    user_id: string;
    user_email: string;
    tier: string;
    theme: string;
    markdown_view: string;
    language: string;
    editor_font_size: number;
    chat_font_size: number;
}

export interface LocalAiConfig {
    ollama_enabled: boolean;
    ollama_url: string;
    ollama_cloud_enabled: boolean;
    ollama_cloud_api_key: string;
    openai_compat_enabled: boolean;
    openai_compat_url: string;
    hidden_local_models: string[];
}

export interface BackendSettings {
    storage: {
        mode: 'local' | 'server';
        sync_metadata: boolean;
        cache: {
            enabled: boolean;
            max_size_mb: number;
        };
    };
    context: {
        max_tokens: number;
        compression: {
            enabled: boolean;
            model: 'local' | 'remote';
        };
    };
    privacy: {
        telemetry: boolean;
    };
    editor: {};
    allow_gitignored_files: boolean;
    auto_approve_run_commands: boolean;
    warmup_context_prefetch: boolean;
}
