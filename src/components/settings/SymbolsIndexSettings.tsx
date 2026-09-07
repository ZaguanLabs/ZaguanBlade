import { useTranslation } from 'react-i18next';
import type { IntegrationConfig, WorkspaceIntegrationSettings } from '../../types/integrations';
import { useSymbolsIndexStatus } from '../../hooks/useSymbolsIndexStatus';

export function SymbolsIndexSettings({ config, workspace, workspacePath, onChange, onWorkspaceChange }: {
    config: IntegrationConfig;
    workspace?: WorkspaceIntegrationSettings;
    workspacePath?: string | null;
    onChange: (config: IntegrationConfig) => void;
    onWorkspaceChange?: (workspace: WorkspaceIntegrationSettings) => void;
}) {
    const { t } = useTranslation();
    const { status, failed } = useSymbolsIndexStatus(workspacePath);
    const override = workspace?.symbols_index_enabled;
    return <section className="space-y-3 rounded-lg border border-(--border-default) bg-(--bg-panel) p-4">
        <h4 className="text-sm font-semibold text-(--fg-primary)">{t('settings.integrations.index.title')}</h4>
        <p className="text-sm text-(--fg-secondary)">{t('settings.integrations.index.help')}</p>
        <label className="flex items-center gap-2 text-sm text-(--fg-secondary)">
            <input type="checkbox" checked={config.symbols_index_enabled ?? true} onChange={event => onChange({ ...config, symbols_index_enabled: event.target.checked })} />
            {t('settings.integrations.index.globalDefault')}
        </label>
        {workspacePath && onWorkspaceChange ? <label className="block text-sm text-(--fg-secondary)">
            {t('settings.integrations.index.workspaceOverride')}
            <select className="ml-3 rounded-md border border-(--border-default) bg-(--bg-input) px-3 py-2"
                value={override == null ? 'inherit' : override ? 'enabled' : 'disabled'}
                onChange={event => onWorkspaceChange({ disabled_ids: [], ...workspace, symbols_index_enabled: event.target.value === 'inherit' ? null : event.target.value === 'enabled' })}>
                {(['inherit', 'enabled', 'disabled'] as const).map(value => <option key={value} value={value}>{t(`settings.integrations.index.${value}`)}</option>)}
            </select>
        </label> : null}
        <p className="text-xs text-(--fg-tertiary)">{t('settings.integrations.index.saveHelp')}</p>
        {workspacePath ? <p role="status" className="text-xs text-(--fg-secondary)">{t('settings.integrations.index.currentStatus', {
            status: t(`settings.integrations.index.${failed ? 'unavailable' : status?.health.status === 'stopping' ? 'stopping' : status ? status.enabled ? 'enabled' : 'disabled' : 'checking'}`),
        })}</p> : null}
    </section>;
}
