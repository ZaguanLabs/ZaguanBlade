import type { BackendSettings, LocalAiConfig, RemoteAiConfig } from '../types/settings';
import type { IntegrationConfig, IntegrationConfigSnapshot } from '../types/integrations';

export interface SettingsPayload {
    remote: RemoteAiConfig;
    local: LocalAiConfig;
    project: BackendSettings;
    integrations: IntegrationConfig;
}

export interface SettingsSaveServices {
    invoke<T>(command: string, args: Record<string, unknown>): Promise<T>;
    emit(event: string): Promise<void>;
    changeLanguage(language: string): Promise<unknown>;
    integrationSaved(revision: string): void;
    refreshModels?: () => Promise<unknown>;
}

/** Rejects on persistence or reconciliation failure. The caller keeps the modal
 * and draft open until this resolves, and restores its busy state in finally. */
export async function saveSettingsChanges(
    next: SettingsPayload,
    previous: SettingsPayload,
    workspacePath: string | null | undefined,
    integrationRevision: string,
    services: SettingsSaveServices,
): Promise<void> {
    const changed = (a: unknown, b: unknown) => JSON.stringify(a) !== JSON.stringify(b);
    const remoteChanged = changed(next.remote, previous.remote);
    const localChanged = changed(next.local, previous.local);
    const projectChanged = Boolean(workspacePath) && changed(next.project, previous.project);
    const integrationsChanged = changed(next.integrations, previous.integrations);

    if (remoteChanged) await services.invoke('save_remote_ai_settings', { settings: next.remote });
    if (localChanged) await services.invoke('save_local_ai_settings', { settings: next.local });
    const saveIntegrations = async () => {
        if (integrationsChanged) {
            const saved = await services.invoke<IntegrationConfigSnapshot>('save_integration_settings', {
                expectedRevision: integrationRevision, config: next.integrations,
            });
            // Advance the revision before notifications: a notification failure must
            // not make the next attempt conflict with our own successful disk write.
            services.integrationSaved(saved.revision);
        }
    };
    // Persist a disabling global default before clearing a workspace override,
    // so the intermediate effective policy cannot start an unwanted index.
    if (next.integrations.symbols_index_enabled === false) await saveIntegrations();
    if (projectChanged) await services.invoke('save_project_settings', { projectPath: workspacePath, settings: next.project });
    if (next.integrations.symbols_index_enabled !== false) await saveIntegrations();

    if (next.remote.theme !== previous.remote.theme) await services.emit('theme-changed');
    if (next.remote.language !== previous.remote.language) await services.changeLanguage(next.remote.language);
    const { theme: _nextTheme, ...nextRemotePreferences } = next.remote;
    const { theme: _previousTheme, ...previousRemotePreferences } = previous.remote;
    if (changed(nextRemotePreferences, previousRemotePreferences)) await services.emit('remote-settings-changed');
    if (localChanged) await services.emit('local-ai-settings-changed');
    if (projectChanged) await services.emit('project-settings-changed');
    if (localChanged && services.refreshModels) await services.refreshModels();
}
