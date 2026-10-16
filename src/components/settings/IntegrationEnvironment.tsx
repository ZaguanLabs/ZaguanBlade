import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import type { ConfigValue, IntegrationDefinition } from '../../types/integrations';
import { integrationProbeErrorKey } from '../../utils/integrationProbe';

const inputClass = 'w-full rounded-md border border-(--border-default) bg-(--bg-input) px-3 py-2 text-sm text-(--fg-primary)';
const buttonClass = 'rounded-md border border-(--border-default) px-3 py-2 text-sm text-(--fg-secondary) disabled:opacity-50';

/** Named configuration values. Each value is plain text or a reference to a
 * credential stored for this integration; credential values are never shown. */
function ConfigValues({ values, onChange, labels, validName, initial }: {
    values: Record<string, ConfigValue>;
    onChange: (values: Record<string, ConfigValue>) => void;
    labels: 'environment' | 'headers';
    validName: (name: string) => boolean;
    initial: (name: string) => ConfigValue;
}) {
    const { t } = useTranslation();
    const [name, setName] = useState('');
    // Header names are case-insensitive; environment variable names are not.
    const same = (existing: string) => labels === 'headers' ? existing.toLowerCase() === name.toLowerCase() : existing === name;
    const valid = validName(name) && !Object.keys(values).some(same);
    return <details className="space-y-3">
        <summary className="cursor-pointer text-sm text-(--fg-secondary)">{t(`settings.integrations.${labels}.title`)}</summary>
        <p className="text-xs text-(--fg-tertiary)">{t(`settings.integrations.${labels}.help`)}</p>
        {Object.entries(values).map(([key, value]) => <div key={key} className="space-y-2 rounded-md border border-(--border-default) p-3">
            <div className="flex items-center justify-between gap-2">
                <span className="font-mono text-sm break-all">{key}</span>
                <button type="button" className={buttonClass} aria-label={t('settings.integrations.environment.remove', { name: key })}
                    onClick={() => onChange(Object.fromEntries(Object.entries(values).filter(([name]) => name !== key)))}>{t('settings.integrations.remove')}</button>
            </div>
            <label className="block text-sm text-(--fg-secondary)">{t('settings.integrations.environment.kind')}
                <select className={inputClass} value={value.kind} onChange={event => onChange({ ...values, [key]: event.target.value === 'secret' ? { kind: 'secret', name: key.toLowerCase() } : { kind: 'text', value: '' } })}>
                    <option value="text">{t('settings.integrations.environment.text')}</option>
                    <option value="secret">{t('settings.integrations.environment.secret')}</option>
                </select>
            </label>
            <label className="block text-sm text-(--fg-secondary)">{t(`settings.integrations.environment.${value.kind === 'secret' ? 'secretName' : 'value'}`)}
                <input className={inputClass} spellCheck={false} autoComplete="off" value={value.kind === 'secret' ? value.name : value.value}
                    onChange={event => onChange({ ...values, [key]: value.kind === 'secret' ? { kind: 'secret', name: event.target.value } : { kind: 'text', value: event.target.value } })} />
            </label>
        </div>)}
        <div className="flex items-end gap-2">
            <label className="block flex-1 text-sm text-(--fg-secondary)">{t(`settings.integrations.${labels}.name`)}
                <input className={inputClass} spellCheck={false} autoComplete="off" value={name} onChange={event => setName(event.target.value)} />
            </label>
            <button type="button" className={buttonClass} disabled={!valid || Object.keys(values).length >= (labels === 'headers' ? 64 : 128)} onClick={() => {
                if (!valid) return;
                onChange({ ...values, [name]: initial(name) }); setName('');
            }}>{t(`settings.integrations.${labels}.add`)}</button>
        </div>
    </details>;
}

export function IntegrationEnvironment({ env, onChange }: { env: Record<string, ConfigValue>; onChange: (env: Record<string, ConfigValue>) => void }) {
    return <ConfigValues values={env} onChange={onChange} labels="environment"
        validName={name => /^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && name.length <= 128}
        initial={() => ({ kind: 'text', value: '' })} />;
}

/** Header names the transport sets itself; the backend rejects them too. */
const reservedHeaders = new Set(['accept', 'content-type', 'content-length', 'host', 'connection', 'transfer-encoding',
    'mcp-session-id', 'mcp-protocol-version', 'mcp-method', 'mcp-name', 'last-event-id']);
const credentialHeaders = new Set(['authorization', 'proxy-authorization', 'cookie']);

export function validHeaderName(name: string): boolean {
    const lower = name.toLowerCase();
    return /^[!#$%&'*+.^_`|~0-9A-Za-z-]{1,128}$/.test(name) && !reservedHeaders.has(lower) && !lower.startsWith('mcp-param-');
}

export function IntegrationHeaders({ headers, onChange }: { headers: Record<string, ConfigValue>; onChange: (headers: Record<string, ConfigValue>) => void }) {
    // Authentication headers must reference protected storage, never plain text.
    return <ConfigValues values={headers} onChange={onChange} labels="headers" validName={validHeaderName}
        initial={name => credentialHeaders.has(name.toLowerCase()) ? { kind: 'secret', name: name.toLowerCase() } : { kind: 'text', value: '' }} />;
}

function SecretField({ integrationId, name, revision, canSave, onStored }: { integrationId: string; name: string; revision?: string | null; canSave?: boolean; onStored: () => void }) {
    const { t } = useTranslation();
    const [value, setValue] = useState('');
    const [busy, setBusy] = useState(false);
    const [status, setStatus] = useState<string | null>(null);
    const generation = useRef(0);
    useEffect(() => {
        setValue(''); setStatus(null); setBusy(false);
        generation.current++;
        return () => { generation.current++; };
    }, [integrationId, name, revision, canSave]);
    const save = async (remove: boolean) => {
        if (!canSave || !revision || busy) return;
        const current = generation.current;
        setBusy(true); setStatus(null);
        try {
            await invoke('set_integration_secret', { integrationId, expectedRevision: revision, name, value: remove ? null : value });
            if (generation.current === current) {
                setValue(''); setStatus(`settings.integrations.environment.${remove ? 'deleted' : 'stored'}`); onStored();
            }
        } catch (failure) {
            if (generation.current === current) setStatus(integrationProbeErrorKey(failure));
        } finally { if (generation.current === current) setBusy(false); }
    };
    return <div className="space-y-2">
        <label className="block text-sm text-(--fg-secondary)">{t('settings.integrations.environment.credential', { name })}
            <input type="password" className={inputClass} autoComplete="new-password" spellCheck={false} value={value}
                disabled={!canSave || busy} onChange={event => setValue(event.target.value)} />
        </label>
        <div className="flex gap-2">
            <button type="button" className={buttonClass} disabled={!canSave || busy || !value} onClick={() => void save(false)}>{t('settings.integrations.environment.store')}</button>
            <button type="button" className={buttonClass} disabled={!canSave || busy} onClick={() => void save(true)}>{t('settings.integrations.environment.delete')}</button>
        </div>
        {status ? <p role="status" className="text-xs text-(--fg-secondary)">{t(status)}</p> : null}
    </div>;
}

export function IntegrationSecrets({ entry, revision, canSave, onStored }: { entry: IntegrationDefinition; revision?: string | null; canSave?: boolean; onStored: () => void }) {
    const { t } = useTranslation();
    const connection = entry.connection;
    const values = connection.protocol === 'acp' ? connection.process.env
        : connection.transport.type === 'stdio' ? connection.transport.process.env : connection.transport.headers;
    const names = [...new Set(Object.values(values).flatMap(value => value.kind === 'secret' ? [value.name] : []))].filter(Boolean);
    if (!names.length) return null;
    return <details className="space-y-3">
        <summary className="cursor-pointer text-sm text-(--fg-secondary)">{t('settings.integrations.environment.credentials')}</summary>
        <p className="text-xs text-(--fg-tertiary)">{t('settings.integrations.environment.credentialsHelp')}</p>
        {!canSave ? <p className="text-xs text-(--fg-tertiary)">{t('settings.integrations.environment.saveFirst')}</p> : null}
        {names.map(name => <SecretField key={name} integrationId={entry.id} name={name} revision={revision} canSave={canSave} onStored={onStored} />)}
    </details>;
}
