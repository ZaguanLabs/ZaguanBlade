import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Trash2 } from 'lucide-react';
import type { IntegrationConfig, IntegrationDefinition, IntegrationProcess } from '../../types/integrations';
import { newIntegrationProcess, removeIntegration, replaceIntegration } from '../../utils/integrationSettings';

const inputClass = 'w-full rounded-md border border-(--border-default) bg-(--bg-input) px-3 py-2 text-sm text-(--fg-primary) focus:outline-none focus:border-(--border-focus)';
const buttonClass = 'inline-flex items-center gap-2 rounded-md border border-(--border-default) px-3 py-2 text-sm text-(--fg-secondary) hover:bg-(--bg-surface-hover) disabled:opacity-50';

function ProcessFields({ process, onChange }: { process: IntegrationProcess; onChange: (process: IntegrationProcess) => void }) {
    const { t } = useTranslation();
    const id = useId();
    return <div className="space-y-3">
        <label className="block text-sm text-(--fg-secondary)">
            {t('settings.integrations.command')}
            <input className={`${inputClass} mt-1`} value={process.command} spellCheck={false}
                onChange={event => onChange({ ...process, command: event.target.value })} />
        </label>
        <div>
            <p id={`${id}-args`} className="text-sm text-(--fg-secondary)">{t('settings.integrations.arguments')}</p>
            <p className="mt-1 text-xs text-(--fg-tertiary)">{t('settings.integrations.argumentsHelp')}</p>
            <div className="mt-2 space-y-2" role="group" aria-labelledby={`${id}-args`}>
                {process.args.map((argument, index) => <div key={index} className="flex gap-2">
                    <input className={inputClass} value={argument} spellCheck={false}
                        aria-label={t('settings.integrations.argumentNumber', { number: index + 1 })}
                        onChange={event => onChange({ ...process, args: process.args.map((value, i) => i === index ? event.target.value : value) })} />
                    <button type="button" className={buttonClass} aria-label={t('settings.integrations.removeArgument', { number: index + 1 })}
                        onClick={() => onChange({ ...process, args: process.args.filter((_, i) => i !== index) })}>
                        <Trash2 className="h-4 w-4" aria-hidden="true" />
                    </button>
                </div>)}
                <button type="button" className={buttonClass} onClick={() => onChange({ ...process, args: [...process.args, ''] })}>
                    <Plus className="h-4 w-4" aria-hidden="true" />{t('settings.integrations.addArgument')}
                </button>
            </div>
        </div>
        <label className="block text-sm text-(--fg-secondary)">
            {t('settings.integrations.directory')}
            <input className={`${inputClass} mt-1`} value={process.cwd ?? ''} spellCheck={false}
                placeholder={t('settings.integrations.workspaceDirectory')}
                onChange={event => onChange({ ...process, cwd: event.target.value || null })} />
        </label>
    </div>;
}

function DefinitionFields({ entry, onChange, onRemove, config }: {
    entry: IntegrationDefinition;
    onChange: (entry: IntegrationDefinition) => void;
    onRemove: () => void;
    config: IntegrationConfig;
}) {
    const { t } = useTranslation();
    const connection = entry.connection;
    return <section className="rounded-lg border border-(--border-default) bg-(--bg-panel) p-4 space-y-4">
        <div className="flex items-center justify-between gap-3">
            <span className="text-xs text-(--fg-tertiary)">{t('settings.integrations.notConnected')}</span>
            <button type="button" className={buttonClass} onClick={onRemove} aria-label={t('settings.integrations.removeDefinition', { name: entry.name })}>
                <Trash2 className="h-4 w-4" aria-hidden="true" />{t('settings.integrations.remove')}
            </button>
        </div>
        <label className="block text-sm text-(--fg-secondary)">
            {t('settings.integrations.name')}
            <input className={`${inputClass} mt-1`} value={entry.name} maxLength={128}
                onChange={event => onChange({ ...entry, name: event.target.value })} />
        </label>
        {connection.protocol === 'mcp' ? <>
            <label className="block text-sm text-(--fg-secondary)">
                {t('settings.integrations.transport')}
                <select className={`${inputClass} mt-1`} value={connection.transport.type}
                    onChange={event => onChange({ ...entry, connection: { protocol: 'mcp', transport: event.target.value === 'stdio'
                        ? { type: 'stdio', process: newIntegrationProcess() }
                        : { type: 'streamable_http', url: '', headers: {} } } })}>
                    <option value="stdio">{t('settings.integrations.stdio')}</option>
                    <option value="streamable_http">{t('settings.integrations.http')}</option>
                </select>
            </label>
            {connection.transport.type === 'stdio'
                ? <ProcessFields process={connection.transport.process} onChange={process => onChange({ ...entry, connection: { protocol: 'mcp', transport: { type: 'stdio', process } } })} />
                : <label className="block text-sm text-(--fg-secondary)">
                    {t('settings.integrations.url')}
                    <input className={`${inputClass} mt-1`} type="url" value={connection.transport.url} spellCheck={false}
                        onChange={event => {
                            if (connection.transport.type === 'streamable_http') onChange({ ...entry, connection: { protocol: 'mcp', transport: { ...connection.transport, url: event.target.value } } });
                        }} />
                </label>}
        </> : <>
            <ProcessFields process={connection.process} onChange={process => onChange({ ...entry, connection: { ...connection, process } })} />
            {config.entries.some(candidate => candidate.connection.protocol === 'mcp') ? <fieldset className="space-y-2">
                <legend className="mb-2 text-sm text-(--fg-secondary)">{t('settings.integrations.forwardedServers')}</legend>
                {config.entries.filter(candidate => candidate.connection.protocol === 'mcp').map(server => <label key={server.id} className="flex gap-2 text-sm text-(--fg-secondary)">
                    <input type="checkbox" checked={connection.mcp_servers.includes(server.id)}
                        onChange={event => onChange({ ...entry, connection: { ...connection, mcp_servers: event.target.checked
                            ? [...connection.mcp_servers, server.id] : connection.mcp_servers.filter(id => id !== server.id) } })} />
                    {server.name}
                </label>)}
            </fieldset> : null}
        </>}
    </section>;
}

export function IntegrationSettings({ config, onChange }: { config: IntegrationConfig; onChange: (config: IntegrationConfig) => void }) {
    const { t } = useTranslation();
    const add = (protocol: 'mcp' | 'acp', atlas = false) => {
        const entry: IntegrationDefinition = {
            id: crypto.randomUUID(),
            name: atlas ? 'Atlas Scout' : t(`settings.integrations.${protocol === 'mcp' ? 'newServer' : 'newAgent'}`),
            enabled: false,
            connection: protocol === 'mcp'
                ? { protocol: 'mcp', transport: { type: 'stdio', process: atlas ? newIntegrationProcess('atlas-scout', ['mcp']) : newIntegrationProcess() } }
                : { protocol: 'acp', process: newIntegrationProcess(), mcp_servers: [] },
        };
        onChange({ ...config, entries: [...config.entries, entry] });
    };
    return <div className="space-y-6">
        <div>
            <h3 className="text-base font-semibold text-(--fg-primary)">{t('settings.integrations.title')}</h3>
            <p className="mt-2 text-sm text-(--fg-secondary)">{t('settings.integrations.description')}</p>
            <p className="mt-2 text-xs text-(--fg-tertiary)">{t('settings.integrations.scope')}</p>
        </div>
        {(['mcp', 'acp'] as const).map(protocol => <div key={protocol} className="space-y-3">
            <h4 className="text-sm font-semibold text-(--fg-primary)">{t(`settings.integrations.${protocol === 'mcp' ? 'servers' : 'agents'}`)}</h4>
            {config.entries.filter(entry => entry.connection.protocol === protocol).map(entry => <DefinitionFields key={entry.id}
                entry={entry} config={config} onChange={updated => onChange(replaceIntegration(config, updated))}
                onRemove={() => onChange(removeIntegration(config, entry.id))} />)}
            {!config.entries.some(entry => entry.connection.protocol === protocol) ? <p className="text-sm text-(--fg-tertiary)">
                {t(`settings.integrations.${protocol === 'mcp' ? 'noServers' : 'noAgents'}`)}
            </p> : null}
            <div className="flex flex-wrap gap-2">
                <button type="button" className={buttonClass} onClick={() => add(protocol)}>
                    <Plus className="h-4 w-4" aria-hidden="true" />{t(`settings.integrations.${protocol === 'mcp' ? 'addServer' : 'addAgent'}`)}
                </button>
                {protocol === 'mcp' ? <button type="button" className={buttonClass} onClick={() => add('mcp', true)}>{t('settings.integrations.addAtlas')}</button> : null}
            </div>
        </div>)}
    </div>;
}
