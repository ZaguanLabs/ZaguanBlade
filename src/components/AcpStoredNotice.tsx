import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import type { AcpAgentStatus } from '../types/acp';
import type { IntegrationConfigSnapshot } from '../types/integrations';
import type { StoredAgentConversation } from '../hooks/useAcpConversation';
import { useAcpLaunch } from '../hooks/useAcpLaunch';
import { enabledAgents } from './AcpAgentMenu';

/** A stored conversation an agent ran. Reconnecting launches the same agent
 * again, after approval, and continues its earlier session when it can. */
export function AcpStoredNotice({ stored, workspaceRoot, onReconnected }: {
    stored: StoredAgentConversation;
    workspaceRoot?: string | null;
    onReconnected: (status: AcpAgentStatus) => void;
}) {
    const { t } = useTranslation();
    const { prepare, starting, error, setError, modal } = useAcpLaunch(onReconnected);
    const reconnect = async () => {
        if (!workspaceRoot) return;
        setError(null);
        try {
            const snapshot = await invoke<IntegrationConfigSnapshot>('get_integration_settings');
            const entry = enabledAgents(snapshot).find(agent => agent.id === stored.agentId);
            if (!entry) { setError('chat.agent.reconnect.unavailable'); return; }
            await prepare(entry, snapshot.revision, workspaceRoot, stored.conversationId);
        } catch { setError('chat.agent.loadFailed'); }
    };
    return <div role="status" className="mx-3 mt-2 rounded-[calc(var(--panel-radius)*0.65)] border border-(--border-subtle) bg-(--bg-surface) px-3 py-2 text-xs">
        <p className="m-0 font-medium text-(--fg-primary)">{t('chat.agent.ended.title', { name: stored.name })}</p>
        <p className="m-0 text-(--fg-secondary)">{t('chat.agent.ended.help')}</p>
        <div className="mt-2 flex flex-wrap items-center gap-2">
            <button type="button" disabled={!workspaceRoot || !!starting} onClick={() => void reconnect()}
                className="rounded border border-(--border-default) px-3 py-1.5 text-xs hover:bg-(--bg-surface-hover) disabled:opacity-50">
                {starting ? t('chat.agent.starting', { name: starting }) : t('chat.agent.reconnect.action', { name: stored.name })}
            </button>
        </div>
        {error ? <p role="alert" className="m-0 mt-1 text-(--state-danger)">{t(error)}</p> : null}
        {modal}
    </div>;
}
