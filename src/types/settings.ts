export interface ApiConfig {
    blade_url: string;
    api_key: string;
    user_id: string;
    ollama_enabled: boolean;
    ollama_url: string;
    openai_compat_enabled: boolean;
    openai_compat_url: string;
    theme: string;
    markdown_view: string;
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
}
