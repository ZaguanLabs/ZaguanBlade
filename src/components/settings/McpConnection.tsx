import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import type { IntegrationDefinition, IntegrationLaunchReview, McpCatalog, McpConnectionStatus } from '../../types/integrations';
import type { ConnectionTestContext } from './IntegrationConnectionTest';
import { McpConnectionRequest } from '../../utils/mcpConnection';
import { integrationProbeErrorKey } from '../../utils/integrationProbe';
import { ConfirmModal } from '../ui/Modal';
import { IntegrationToolCatalog } from './IntegrationToolCatalog';

export interface LiveConnectionContext {
    status?: McpConnectionStatus;
    phase: 'loading' | 'ready' | 'failed';
    update: (status: McpConnectionStatus) => void;
}
const buttonClass = 'rounded-md border border-(--border-default) px-3 py-2 text-sm text-(--fg-secondary) hover:bg-(--bg-surface-hover) disabled:opacity-50';
export function McpConnection({ entry, revision, workspacePath, canTest, live }: ConnectionTestContext & { entry: IntegrationDefinition; live: LiveConnectionContext }) {
    const { t } = useTranslation();
    const request = useRef<McpConnectionRequest | null>(null);
    const operation = useRef(0);
    const [review, setReview] = useState<IntegrationLaunchReview | null>(null);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [catalog, setCatalog] = useState<McpCatalog | null>(null);
    const status = live.status;
    useEffect(() => {
        setReview(null); setBusy(false); setError(null);
        return () => { operation.current += 1; request.current?.dispose(); request.current = null; };
    }, [entry, revision, workspacePath, canTest]);
    useEffect(() => {
        let disposed = false;
        setCatalog(null);
        if (live.phase !== 'ready' || status?.phase !== 'connected' || !status.catalog_revision) return;
        void invoke<McpCatalog>('get_mcp_catalog', { connectionId: status.connection_id }).then(next => {
            if (!disposed && next.integration_id === entry.id && next.revision === status.catalog_revision) setCatalog(next);
        }).catch(failure => { if (!disposed) setError(integrationProbeErrorKey(failure)); });
        return () => { disposed = true; };
    }, [entry.id, status?.connection_id, status?.catalog_revision, status?.phase, live.phase]);
    const cancel = () => {
        request.current?.dispose(); request.current = null;
        operation.current += 1; setReview(null); setBusy(false);
    };
    const prepare = async () => {
        if (!canTest || !revision || !workspacePath || request.current || busy) return;
        const current = new McpConnectionRequest(invoke);
        request.current = current; setBusy(true); setError(null);
        try {
            const next = await current.prepare(entry.id, revision, workspacePath);
            if (request.current === current) setReview(next);
        } catch (failure) {
            if (request.current === current) { setError(integrationProbeErrorKey(failure)); cancel(); }
        } finally { if (request.current === current) setBusy(false); }
    };
    const connect = async () => {
        const current = request.current;
        if (!current || busy) return;
        setBusy(true); setReview(null);
        try {
            const next = await current.connect();
            if (request.current === current && next) live.update(next);
        } catch (failure) {
            if (request.current === current) setError(integrationProbeErrorKey(failure));
        } finally {
            if (request.current === current) { request.current = null; setBusy(false); }
        }
    };
    const control = async (command: 'disconnect_mcp' | 'refresh_mcp_catalog') => {
        if (!status || busy) return;
        const current = ++operation.current;
        setBusy(true); setError(null); setCatalog(null);
        live.update({ ...status, phase: command === 'disconnect_mcp' ? 'stopping' : 'refreshing', catalog_revision: null });
        try { await invoke(command, { connectionId: status.connection_id }); }
        catch (failure) { if (operation.current === current) setError(integrationProbeErrorKey(failure)); }
        finally { if (operation.current === current) setBusy(false); }
    };
    const active = status && ['connecting', 'connected', 'refreshing', 'stopping'].includes(status.phase);
    return <section className="space-y-2 border-t border-(--border-default) pt-3">
        <h4 className="text-sm font-semibold text-(--fg-primary)">{t('settings.integrations.connection.title')}</h4>
        <p className="text-xs text-(--fg-tertiary)">{t('settings.integrations.connection.help')}</p>
        <p role="status" className="text-sm text-(--fg-secondary)">{t(`settings.integrations.connection.${live.phase === 'ready' ? status?.phase ?? 'disconnected' : live.phase === 'failed' ? 'statusUnavailable' : 'checking'}`)}</p>
        <div className="flex flex-wrap gap-2">
            {!active ? <button type="button" className={buttonClass} disabled={live.phase !== 'ready' || !entry.enabled || !canTest || !revision || !workspacePath || busy || !!review} onClick={() => void prepare()}>{t('settings.integrations.connection.connect')}</button> : null}
            {active ? <button type="button" className={buttonClass} disabled={busy || status.phase === 'stopping'} onClick={() => void control('disconnect_mcp')}>{t('settings.integrations.connection.disconnect')}</button> : null}
            {status?.phase === 'connected' ? <button type="button" className={buttonClass} disabled={busy || !canTest} onClick={() => void control('refresh_mcp_catalog')}>{t('settings.integrations.connection.refresh')}</button> : null}
            {busy && request.current ? <button type="button" className={buttonClass} onClick={cancel}>{t('common.cancel')}</button> : null}
        </div>
        {!entry.enabled ? <p className="text-xs text-(--fg-tertiary)">{t('settings.integrations.connection.enableFirst')}</p> : null}
        {error || status?.error ? <p role="alert" className="text-sm text-(--state-danger)">{t(error ?? integrationProbeErrorKey(status?.error))}</p> : null}
        {catalog && live.phase === 'ready' && status?.phase === 'connected' && catalog.revision === status.catalog_revision ? <IntegrationToolCatalog catalog={catalog} serverName={entry.name} origin="connection" /> : null}
        <ConfirmModal isOpen={!!review} title={t('settings.integrations.connection.reviewTitle', { name: entry.name })}
            confirmLabel={t('settings.integrations.connection.allow')} onConfirm={() => void connect()} onCancel={cancel}
            message={<div className="space-y-3">
                <p>{t('settings.integrations.connection.reviewHelp')}</p>
                <dl className="max-h-60 overflow-auto break-all">
                    <dt>{t('settings.integrations.command')}</dt><dd className="font-mono text-xs">{review?.executable}</dd>
                    <dt className="mt-2">{t('settings.integrations.arguments')}</dt><dd className="font-mono text-xs whitespace-pre-wrap">{JSON.stringify(review?.args, null, 2)}</dd>
                    <dt className="mt-2">{t('settings.integrations.directory')}</dt><dd className="font-mono text-xs">{review?.cwd}</dd>
                </dl>
            </div>} />
    </section>;
}
