import type { BackendSettings } from '../types/settings';
import type { WorkspaceIntegrationSettings } from '../types/integrations';

type StorageMode = 'local' | 'server';

export interface ProjectPreferences {
    workspaceIntegrations: WorkspaceIntegrationSettings;
    storage: {
        mode: StorageMode;
        syncMetadata: boolean;
        cache: {
            enabled: boolean;
            maxSizeMb: number;
        };
    };
    context: {
        maxTokens: number;
        compression: {
            enabled: boolean;
            model: 'local' | 'remote';
        };
    };
    privacy: {
        telemetry: boolean;
    };
    editor: {};
    skills: BackendSettings['skills'];
    allowGitIgnoredFiles?: boolean;  // Per-project setting
    autoApproveRunCommands?: boolean;
    warmupContextPrefetch?: boolean;  // Per-project setting
}

export function projectSettingsFromBackend(backend: BackendSettings): ProjectPreferences {
    return {
        storage: {
            mode: backend.storage.mode,
            syncMetadata: backend.storage.sync_metadata,
            cache: {
                enabled: backend.storage.cache.enabled,
                maxSizeMb: backend.storage.cache.max_size_mb,
            },
        },
        context: {
            maxTokens: backend.context.max_tokens,
            compression: {
                enabled: backend.context.compression.enabled,
                model: backend.context.compression.model,
            },
        },
        privacy: {
            telemetry: backend.privacy.telemetry,
        },
        editor: {},
        skills: backend.skills ?? { config: [] },
        workspaceIntegrations: backend.integrations ?? { disabled_ids: [] },
        allowGitIgnoredFiles: backend.allow_gitignored_files,
        autoApproveRunCommands: backend.auto_approve_run_commands,
        warmupContextPrefetch: backend.warmup_context_prefetch ?? true,
    };
}

export function projectSettingsToBackend(frontend: ProjectPreferences): BackendSettings {
    return {
        storage: {
            mode: frontend.storage.mode,
            sync_metadata: frontend.storage.syncMetadata,
            cache: {
                enabled: frontend.storage.cache.enabled,
                max_size_mb: frontend.storage.cache.maxSizeMb,
            },
        },
        context: {
            max_tokens: frontend.context.maxTokens,
            compression: {
                enabled: frontend.context.compression.enabled,
                model: frontend.context.compression.model,
            },
        },
        privacy: {
            telemetry: false,
        },
        editor: {},
        skills: frontend.skills,
        integrations: frontend.workspaceIntegrations,
        allow_gitignored_files: frontend.allowGitIgnoredFiles || false,
        auto_approve_run_commands: frontend.autoApproveRunCommands || false,
        warmup_context_prefetch: frontend.warmupContextPrefetch ?? true,
    };
}
