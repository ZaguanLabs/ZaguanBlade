import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { ConfirmModal } from '../ui/Modal';
import type { IntegrationDefinition, IntegrationLaunchReview, IntegrationProbeResult } from '../../types/integrations';
import { IntegrationProbeRequest, integrationProbeErrorKey } from '../../utils/integrationProbe';

export interface ConnectionTestContext {
    revision?: string | null;
    workspacePath?: string | null;
    canTest?: boolean;
}

const buttonClass = 'rounded-md border border-(--border-default) px-3 py-2 text-sm text-(--fg-secondary) hover:bg-(--bg-surface-hover) disabled:opacity-50';

export function IntegrationConnectionTest({ entry, revision, workspacePath, canTest }: ConnectionTestContext & { entry: IntegrationDefinition }) {
    const { t } = useTranslation();
    const request = useRef<IntegrationProbeRequest | null>(null);
    const launching = useRef(false);
    const [review, setReview] = useState<IntegrationLaunchReview | null>(null);
    const [busy, setBusy] = useState(false);
    const [result, setResult] = useState<IntegrationProbeResult | null>(null);
    const [error, setError] = useState<string | null>(null);
    const isHttp = entry.connection.protocol === 'mcp' && entry.connection.transport.type === 'streamable_http';

    useEffect(() => {
        setReview(null); setBusy(false); setResult(null); setError(null);
        return () => { request.current?.dispose(); request.current = null; launching.current = false; };
    }, [entry, revision, workspacePath, canTest]);

    const cancel = () => {
        request.current?.dispose(); request.current = null;
        launching.current = false;
        setReview(null); setBusy(false);
    };
    const prepare = async () => {
        if (!canTest || !revision || !workspacePath || request.current) return;
        const current = new IntegrationProbeRequest(invoke);
        request.current = current;
        setBusy(true); setError(null); setResult(null);
        try {
            const next = await current.prepare(entry.id, revision, workspacePath);
            if (request.current === current) setReview(next);
        } catch (failure) {
            if (request.current === current) { setError(integrationProbeErrorKey(failure)); cancel(); }
        } finally { if (request.current === current) setBusy(false); }
    };
    const run = async () => {
        const current = request.current;
        if (!current || busy || launching.current) return;
        launching.current = true;
        setReview(null); setBusy(true);
        try {
            const next = await current.run();
            if (request.current === current) setResult(next);
        } catch (failure) {
            if (request.current === current) setError(integrationProbeErrorKey(failure));
        } finally {
            if (request.current === current) { request.current = null; launching.current = false; setBusy(false); }
        }
    };

    if (isHttp) return <p className="text-xs text-(--fg-tertiary)">{t('settings.integrations.test.httpPending')}</p>;
    return <div className="space-y-2 border-t border-(--border-default) pt-3">
        <div className="flex items-center gap-2">
            <button type="button" className={buttonClass} disabled={!canTest || !revision || !workspacePath || busy || !!review} onClick={() => void prepare()}>
                {t(`settings.integrations.test.${busy ? 'testing' : 'button'}`)}
            </button>
            {busy ? <button type="button" className={buttonClass} onClick={cancel}>{t('common.cancel')}</button> : null}
        </div>
        {!workspacePath || !canTest ? <p className="text-xs text-(--fg-tertiary)">{t(`settings.integrations.test.${!workspacePath ? 'workspaceNeeded' : 'saveFirst'}`)}</p> : null}
        {error ? <p role="alert" className="text-sm text-(--state-danger)">{t(error)}</p> : null}
        {result ? <div role="status" className="text-sm text-(--fg-secondary)">
            <p>{t('settings.integrations.test.success', { version: result.protocol_version })}</p>
            <p>{entry.connection.protocol === 'mcp'
                ? t('settings.integrations.test.mcpResult', { count: result.tools, resources: t(result.resources ? 'common.yes' : 'common.no'), prompts: t(result.prompts ? 'common.yes' : 'common.no') })
                : t('settings.integrations.test.acpResult', { count: result.authentication_methods })}</p>
        </div> : null}
        <ConfirmModal isOpen={!!review} title={t('settings.integrations.test.reviewTitle', { name: entry.name })}
            confirmLabel={t('settings.integrations.test.allowOnce')} onConfirm={() => void run()} onCancel={cancel}
            message={<div className="space-y-3">
                <p>{t('settings.integrations.test.reviewHelp')}</p>
                <dl className="max-h-60 overflow-auto break-all">
                    <dt>{t('settings.integrations.command')}</dt><dd className="font-mono text-xs">{review?.executable}</dd>
                    <dt className="mt-2">{t('settings.integrations.arguments')}</dt><dd className="font-mono text-xs whitespace-pre-wrap">{JSON.stringify(review?.args, null, 2)}</dd>
                    <dt className="mt-2">{t('settings.integrations.directory')}</dt><dd className="font-mono text-xs">{review?.cwd}</dd>
                </dl>
            </div>} />
    </div>;
}
