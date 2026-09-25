import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { integrationProbeErrorKey } from '../utils/integrationProbe';

export interface McpTurnState {
    turn_id: string;
    conversation_id: string;
    available: number;
    limit: number;
    excluded: Array<{ server_name: string; tool_name: string; reason: 'schema' | 'budget' }>;
    blocked: 'remote' | 'provider' | 'budget' | 'uncertain' | 'model_changed' | null;
    pending: Array<{ request_id: string; server_name: string; tool_name: string; arguments: unknown }>;
}
const buttonClass = 'rounded border border-(--border-default) px-3 py-1.5 hover:bg-(--bg-surface-hover) disabled:opacity-50';
export function McpTurnView({ state, busy, error, respond }: { state: McpTurnState; busy: boolean; error: string | null; respond: (request: string, allow: boolean) => void }) {
    const { t } = useTranslation();
    if (!state.available && !state.excluded.length && !state.blocked && !error) return null;
    return <section aria-label={t('mcpChat.title')} className="max-h-[45%] shrink-0 overflow-auto border-b border-(--border-default) px-4 py-2 text-xs text-(--fg-secondary)">
        <details open={state.excluded.length > 0 || undefined}>
            <summary className="cursor-pointer">{t('mcpChat.catalog', { count: state.available, limit: state.limit })}</summary>
            <p className="my-2">{t('mcpChat.catalogHelp')}</p>
            {state.excluded.map((tool, index) => <p key={index}>{tool.server_name} / {tool.tool_name}: {t(`mcpChat.excluded.${tool.reason}`)}</p>)}
        </details>
        {state.blocked && <p role="status" className="mt-2">{t(`mcpChat.blocked.${state.blocked}`)}</p>}
        {error && <p role="alert" className="mt-2 text-(--status-error)">{t(error)}</p>}
        {state.pending.map(review => <div key={review.request_id} className="mt-3 rounded border border-(--border-default) p-3">
            <p className="font-medium text-(--fg-primary)">{t('mcpChat.approval', { server: review.server_name, tool: review.tool_name })}</p>
            <p className="my-2">{t('mcpChat.approvalHelp')}</p>
            <details><summary className="cursor-pointer">{t('mcpChat.arguments')}</summary><pre className="my-2 max-h-48 overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(review.arguments, null, 2)}</pre></details>
            <div className="mt-2 flex gap-2">
                <button className={buttonClass} disabled={busy} onClick={() => respond(review.request_id, false)}>{t('mcpChat.deny')}</button>
                <button className={buttonClass} disabled={busy} onClick={() => respond(review.request_id, true)}>{t('mcpChat.allow')}</button>
            </div>
        </div>)}
    </section>;
}
export function McpTurnPanel({ workspaceRoot }: { workspaceRoot?: string | null }) {
    const [state, setState] = useState<McpTurnState | null>(null);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const epoch = useRef(0);
    const currentTurn = useRef<string | null>(null);
    useEffect(() => {
        let disposed = false;
        let timer: ReturnType<typeof setTimeout>;
        epoch.current += 1;
        currentTurn.current = null;
        setState(null); setError(null); setBusy(false);
        const poll = async () => {
            try {
                const next = await invoke<McpTurnState | null>('get_mcp_turn_state');
                if (!disposed) {
                    if (currentTurn.current !== (next?.turn_id ?? null)) {
                        epoch.current += 1;
                        currentTurn.current = next?.turn_id ?? null;
                        setError(null); setBusy(false);
                    }
                    setState(next);
                }
            } catch {
                if (!disposed) { setState(null); currentTurn.current = null; epoch.current += 1; }
            } finally { if (!disposed) timer = setTimeout(poll, 750); }
        };
        if (workspaceRoot) void poll();
        return () => { disposed = true; epoch.current += 1; clearTimeout(timer); };
    }, [workspaceRoot]);
    const respond = async (requestId: string, allow: boolean) => {
        if (!state || busy) return;
        const generation = epoch.current;
        setBusy(true); setError(null);
        try {
            await invoke('respond_mcp_tool_call', { turnId: state.turn_id, requestId, allow });
            if (epoch.current === generation) setState(previous => previous && ({ ...previous, pending: previous.pending.filter(review => review.request_id !== requestId) }));
        } catch (failure) { if (epoch.current === generation) setError(integrationProbeErrorKey(failure)); }
        finally { if (epoch.current === generation) setBusy(false); }
    };
    return state && <McpTurnView state={state} busy={busy} error={error} respond={respond} />;
}
