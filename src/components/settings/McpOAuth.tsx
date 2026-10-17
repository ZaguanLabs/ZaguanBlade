import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import type { IntegrationDefinition, McpOAuthSettings } from '../../types/integrations';
import { integrationProbeErrorKey } from '../../utils/integrationProbe';

const inputClass = 'w-full rounded-md border border-(--border-default) bg-(--bg-input) px-3 py-2 text-sm text-(--fg-primary) focus:outline-none focus:border-(--border-focus)';
const buttonClass = 'rounded-md border border-(--border-default) px-3 py-2 text-sm text-(--fg-secondary) hover:bg-(--bg-surface-hover) disabled:opacity-50';

/** Space-separated scopes as typed; empty lets the server choose. */
export const parseScopes = (value: string) => value.split(/\s+/).filter(Boolean);

/** OAuth configuration for a Streamable HTTP server, plus sign-in against the
 * saved configuration. Tokens never reach the webview. */
export function McpOAuth({ entry, oauth, onChange, revision, canSave }: {
    entry: IntegrationDefinition;
    oauth: McpOAuthSettings | undefined;
    onChange: (oauth: McpOAuthSettings | undefined) => void;
    revision?: string | null;
    canSave?: boolean;
}) {
    const { t } = useTranslation();
    const [scopes, setScopes] = useState((oauth?.scopes ?? []).join(' '));
    const [signedIn, setSignedIn] = useState<boolean | null>(null);
    const [busy, setBusy] = useState<'sign_in' | 'sign_out' | null>(null);
    const [error, setError] = useState<string | null>(null);
    const generation = useRef(0);
    const signingIn = useRef(false);
    const enabled = oauth !== undefined;
    const ready = enabled && !!canSave && !!revision;

    useEffect(() => {
        setSignedIn(null);
        if (!ready) return;
        const current = ++generation.current;
        invoke<boolean>('get_mcp_sign_in_status', { integrationId: entry.id })
            .then(next => { if (current === generation.current) setSignedIn(next); })
            .catch(failure => { if (current === generation.current) setError(integrationProbeErrorKey(failure)); });
    }, [entry.id, ready, revision]);
    // Leaving Settings mid sign-in stops this form's wait for the browser.
    useEffect(() => () => {
        generation.current += 1;
        if (signingIn.current) void invoke('cancel_mcp_sign_in').catch(() => {});
    }, []);

    const signIn = async () => {
        if (!ready || busy) return;
        const current = ++generation.current;
        setBusy('sign_in'); setError(null);
        signingIn.current = true;
        try {
            await invoke('sign_in_mcp_server', {
                integrationId: entry.id,
                expectedRevision: revision,
                success: t('settings.integrations.oauth.browserSuccess'),
                failure: t('settings.integrations.oauth.browserFailure'),
            });
            if (current === generation.current) setSignedIn(true);
        } catch (failure) {
            if (current === generation.current && failure !== 'cancelled') setError(integrationProbeErrorKey(failure));
        } finally {
            signingIn.current = false;
            if (current === generation.current) setBusy(null);
        }
    };
    const signOut = async () => {
        if (busy) return;
        const current = ++generation.current;
        setBusy('sign_out'); setError(null);
        try {
            await invoke('sign_out_mcp_server', { integrationId: entry.id });
            if (current === generation.current) setSignedIn(false);
        } catch (failure) {
            if (current === generation.current) setError(integrationProbeErrorKey(failure));
        } finally {
            if (current === generation.current) setBusy(null);
        }
    };
    const cancel = () => {
        generation.current += 1;
        setBusy(null);
        void invoke('cancel_mcp_sign_in').catch(() => {});
    };

    return <fieldset className="space-y-3 rounded-md border border-(--border-subtle) p-3">
        <legend className="px-1 text-sm text-(--fg-secondary)">{t('settings.integrations.oauth.title')}</legend>
        <label className="flex gap-2 text-sm text-(--fg-secondary)">
            <input type="checkbox" checked={enabled} onChange={event => onChange(event.target.checked ? {} : undefined)} />
            {t('settings.integrations.oauth.enable')}
        </label>
        <p className="text-xs text-(--fg-tertiary)">{t('settings.integrations.oauth.help')}</p>
        {enabled ? <>
            <label className="block text-sm text-(--fg-secondary)">
                {t('settings.integrations.oauth.clientId')}
                <input className={`${inputClass} mt-1`} value={oauth?.client_id ?? ''} spellCheck={false} maxLength={256}
                    placeholder={t('settings.integrations.oauth.clientIdPlaceholder')}
                    onChange={event => onChange({ ...oauth, client_id: event.target.value.trim() || undefined })} />
            </label>
            <label className="block text-sm text-(--fg-secondary)">
                {t('settings.integrations.oauth.scopes')}
                <input className={`${inputClass} mt-1`} value={scopes} spellCheck={false}
                    placeholder={t('settings.integrations.oauth.scopesPlaceholder')}
                    onChange={event => {
                        setScopes(event.target.value);
                        const next = parseScopes(event.target.value);
                        onChange({ ...oauth, scopes: next.length ? next : undefined });
                    }} />
            </label>
            {!canSave || !revision ? <p className="text-xs text-(--fg-tertiary)">{t('settings.integrations.oauth.saveFirst')}</p> : null}
            {ready ? <p role="status" className="text-sm text-(--fg-secondary)">
                {busy === 'sign_in' ? t('settings.integrations.oauth.waiting')
                    : signedIn === null ? t('settings.integrations.oauth.checking')
                        : t(signedIn ? 'settings.integrations.oauth.signedIn' : 'settings.integrations.oauth.signedOut')}
            </p> : null}
            <div className="flex flex-wrap gap-2">
                {busy === 'sign_in'
                    ? <button type="button" className={buttonClass} onClick={cancel}>{t('common.cancel')}</button>
                    : <button type="button" className={buttonClass} disabled={!ready || !!busy} onClick={() => void signIn()}>
                        {t(signedIn ? 'settings.integrations.oauth.signInAgain' : 'settings.integrations.oauth.signIn')}
                    </button>}
                {signedIn ? <button type="button" className={buttonClass} disabled={!!busy} onClick={() => void signOut()}>{t('settings.integrations.oauth.signOut')}</button> : null}
            </div>
            {error ? <p role="alert" className="text-sm text-(--state-danger)">{t(error)}</p> : null}
        </> : null}
    </fieldset>;
}
