import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { Bot } from 'lucide-react';
import type { AcpAgentStatus } from '../types/acp';
import type { IntegrationConfigSnapshot, IntegrationDefinition, IntegrationLaunchReview } from '../types/integrations';
import { AcpStartRequest } from '../utils/acpConversation';
import { integrationProbeErrorKey } from '../utils/integrationProbe';
import { LaunchReviewDetails } from './settings/IntegrationLaunchReview';
import { ConfirmModal } from './ui/Modal';

/** Enabled ACP agents from the saved configuration. Workspace policy is
 * enforced again by the backend when the launch is prepared. */
export function enabledAgents(snapshot: IntegrationConfigSnapshot): IntegrationDefinition[] {
    return snapshot.config.entries.filter(entry => entry.enabled && entry.connection.protocol === 'acp');
}

/** Starts a conversation run by an ACP agent, after the user approves the
 * exact program launch. */
export function AcpAgentMenu({ workspaceRoot, onStarted }: { workspaceRoot?: string | null; onStarted: (status: AcpAgentStatus) => void }) {
    const { t } = useTranslation();
    const [open, setOpen] = useState(false);
    const [snapshot, setSnapshot] = useState<IntegrationConfigSnapshot | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [review, setReview] = useState<{ name: string; review: IntegrationLaunchReview } | null>(null);
    const [starting, setStarting] = useState<string | null>(null);
    const request = useRef<AcpStartRequest | null>(null);
    const container = useRef<HTMLDivElement>(null);

    useEffect(() => () => { request.current?.dispose(); request.current = null; }, []);
    useEffect(() => {
        if (!open) return;
        let disposed = false;
        setError(null);
        invoke<IntegrationConfigSnapshot>('get_integration_settings')
            .then(next => { if (!disposed) setSnapshot(next); })
            .catch(() => { if (!disposed) setError('chat.agent.loadFailed'); });
        const close = (event: MouseEvent | KeyboardEvent) => {
            if (event instanceof KeyboardEvent ? event.key === 'Escape' : !container.current?.contains(event.target as Node)) setOpen(false);
        };
        document.addEventListener('mousedown', close);
        document.addEventListener('keydown', close);
        return () => { disposed = true; document.removeEventListener('mousedown', close); document.removeEventListener('keydown', close); };
    }, [open]);

    const reset = () => { request.current?.dispose(); request.current = null; setReview(null); setStarting(null); };
    const prepare = async (entry: IntegrationDefinition) => {
        if (!snapshot || !workspaceRoot || request.current) return;
        const current = new AcpStartRequest(invoke);
        request.current = current;
        setOpen(false); setError(null);
        try {
            const next = await current.prepare(entry.id, snapshot.revision, workspaceRoot);
            if (request.current === current && next) setReview({ name: entry.name, review: next });
        } catch (failure) {
            if (request.current === current) { setError(integrationProbeErrorKey(failure)); reset(); }
        }
    };
    const start = async () => {
        const current = request.current;
        if (!current || !review) return;
        setStarting(review.name); setReview(null);
        try {
            const status = await current.start();
            if (request.current === current && status) onStarted(status);
        } catch (failure) {
            if (request.current === current) setError(integrationProbeErrorKey(failure));
        } finally {
            if (request.current === current) { request.current = null; setStarting(null); }
        }
    };
    const agents = snapshot ? enabledAgents(snapshot) : [];
    return <div ref={container} className="relative">
        <button type="button" onClick={() => setOpen(value => !value)} disabled={!workspaceRoot || !!starting}
            aria-label={t('chat.agent.start')} title={starting ? t('chat.agent.starting', { name: starting }) : t('chat.agent.start')} aria-expanded={open}
            className="inline-flex h-9 w-9 items-center justify-center rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface)/70 text-(--fg-secondary) transition-colors hover:border-[color-mix(in_srgb,var(--accent-ai)_32%,transparent)] hover:text-(--fg-primary) disabled:opacity-50">
            <Bot className={`h-4 w-4 ${starting ? 'animate-pulse' : ''}`} aria-hidden="true" />
        </button>
        {open ? <div role="menu" aria-label={t('chat.agent.menu')}
            className="absolute right-0 top-10 z-50 w-64 rounded-[calc(var(--panel-radius)*0.65)] border border-(--border-subtle) bg-(--bg-panel) p-1 text-xs shadow-(--shadow-sm)">
            {agents.map(entry => <button key={entry.id} type="button" role="menuitem" onClick={() => void prepare(entry)}
                className="block w-full truncate rounded px-2 py-1.5 text-left text-(--fg-primary) hover:bg-(--bg-surface-hover)">{entry.name}</button>)}
            {snapshot && !agents.length ? <div className="space-y-2 p-2 text-(--fg-secondary)">
                <p>{t('chat.agent.none')}</p>
                <button type="button" className="text-(--accent-mention) hover:underline" onClick={() => {
                    setOpen(false);
                    document.dispatchEvent(new CustomEvent('open-settings', { detail: { section: 'integrations' } }));
                }}>{t('chat.agent.openSettings')}</button>
            </div> : null}
            {error && open ? <p role="alert" className="p-2 text-(--state-danger)">{t(error)}</p> : null}
        </div> : null}
        {error && !open ? <p role="alert" className="absolute right-0 top-10 z-40 w-64 rounded border border-(--border-subtle) bg-(--bg-panel) p-2 text-xs text-(--state-danger)"
            onClick={() => setError(null)}>{t(error)}</p> : null}
        <ConfirmModal isOpen={!!review} title={review ? t('chat.agent.reviewTitle', { name: review.name }) : ''}
            confirmLabel={t('chat.agent.allow')} onConfirm={() => void start()} onCancel={reset}
            message={<LaunchReviewDetails review={review?.review ?? null} purpose="agent" />} />
    </div>;
}
