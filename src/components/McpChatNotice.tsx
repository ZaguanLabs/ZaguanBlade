import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { X } from 'lucide-react';
import type { IntegrationConfigSnapshot, IntegrationDefinition, McpConnectionStatus } from '../types/integrations';
import { useMcpConnections } from '../hooks/useMcpConnections';

export type McpChatNoticeState = { kind: 'not_connected'; names: string[] } | { kind: 'cloud_model' } | null;

const LOCAL_PROVIDERS = new Set(['ollama', 'openai-compat']);
const STARTING = new Set(['connecting', 'connected', 'refreshing']);

/** Why enabled MCP servers will not reach the next message, if they won't.
 * Only Ollama and OpenAI-compatible models receive MCP tools, and only from
 * connected servers. */
export function mcpChatNotice(entries: IntegrationDefinition[], statuses: McpConnectionStatus[], provider?: string): McpChatNoticeState {
    const servers = entries.filter(entry => entry.enabled && entry.connection.protocol === 'mcp');
    const phase = (id: string) => statuses.find(status => status.integration_id === id)?.phase;
    if (!LOCAL_PROVIDERS.has(provider ?? '')) {
        return servers.some(server => phase(server.id) === 'connected') ? { kind: 'cloud_model' } : null;
    }
    const idle = servers.filter(server => !STARTING.has(phase(server.id) ?? 'disconnected'));
    return idle.length ? { kind: 'not_connected', names: idle.map(server => server.name) } : null;
}

export function McpChatNoticeView({ notice, onOpenSettings, onDismiss }: { notice: NonNullable<McpChatNoticeState>; onOpenSettings: () => void; onDismiss: () => void }) {
    const { t } = useTranslation();
    return <div role="status" className="flex shrink-0 items-start gap-2 border-b border-(--border-subtle) px-4 py-2 text-xs text-(--fg-secondary)">
        <p className="min-w-0 flex-1">
            {notice.kind === 'cloud_model'
                ? t('mcpChat.notice.cloudModel')
                : t('mcpChat.notice.notConnected', { count: notice.names.length, names: notice.names.join(', ') })}
            {' '}
            <button type="button" className="text-(--accent-mention) hover:underline" onClick={onOpenSettings}>{t('mcpChat.notice.openSettings')}</button>
        </p>
        <button type="button" aria-label={t('common.close')} title={t('common.close')} onClick={onDismiss}
            className="shrink-0 rounded p-0.5 text-(--fg-tertiary) hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)">
            <X className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
    </div>;
}

/** Tells the user before they send when MCP tools won't reach the model. */
export function McpChatNotice({ workspaceRoot, provider }: { workspaceRoot?: string | null; provider?: string }) {
    const live = useMcpConnections(workspaceRoot);
    const [entries, setEntries] = useState<IntegrationDefinition[]>([]);
    const [dismissed, setDismissed] = useState<string | null>(null);
    useEffect(() => {
        let disposed = false;
        const load = () => invoke<IntegrationConfigSnapshot>('get_integration_settings')
            .then(snapshot => { if (!disposed) setEntries(snapshot.config.entries); })
            .catch(() => { if (!disposed) setEntries([]); });
        void load();
        const timer = setInterval(() => void load(), 5000);
        return () => { disposed = true; clearInterval(timer); };
    }, []);
    if (live.phase !== 'ready') return null;
    const notice = mcpChatNotice(entries, live.statuses, provider);
    const key = JSON.stringify(notice);
    if (!notice || dismissed === key) return null;
    return <McpChatNoticeView notice={notice} onDismiss={() => setDismissed(key)}
        onOpenSettings={() => document.dispatchEvent(new CustomEvent('open-settings', { detail: { section: 'integrations' } }))} />;
}
