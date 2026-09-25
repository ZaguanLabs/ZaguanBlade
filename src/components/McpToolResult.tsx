import { useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { integrationProbeErrorKey } from '../utils/integrationProbe';

type ArtifactRef = { conversation_id: string; turn_id: string; artifact_id: string };
type McpResult = { server?: string; tool?: string; result: { outcome: 'completed' | 'unknown' | 'not_started'; error?: string; projection?: { text: string; truncated: boolean; non_text_blocks: number; is_error: boolean }; artifact?: ArtifactRef; artifact_unavailable?: boolean } };
export function parseMcpResult(raw?: string): McpResult | null {
    if (!raw) return null;
    try {
        const value = JSON.parse(raw);
        if (value?.mcp_result_version !== 1 || !['completed', 'unknown', 'not_started'].includes(value.result?.outcome)) return null;
        if (value.server != null && typeof value.server !== 'string' || value.tool != null && typeof value.tool !== 'string') return null;
        const projection = value.result.projection;
        if (projection && (typeof projection.text !== 'string' || typeof projection.truncated !== 'boolean' || typeof projection.non_text_blocks !== 'number')) return null;
        const artifact = value.result.artifact;
        if (artifact && !['conversation_id', 'turn_id', 'artifact_id'].every(key => typeof artifact[key] === 'string')) return null;
        return value;
    } catch { return null; }
}
export function McpToolResult({ alias, raw, status }: { alias: string; raw?: string; status: string }) {
    const { t } = useTranslation();
    const payload = useMemo(() => parseMcpResult(raw), [raw]);
    const [artifact, setArtifact] = useState<string | null>(null);
    const [error, setError] = useState(false);
    const [busy, setBusy] = useState(false);
    const epoch = useRef(0);
    useEffect(() => {
        epoch.current += 1; setArtifact(null); setError(false); setBusy(false);
        return () => { epoch.current += 1; };
    }, [raw]);
    const load = async () => {
        if (!payload?.result.artifact || busy) return;
        const generation = epoch.current;
        setBusy(true); setError(false);
        try {
            const value = await invoke<unknown>('get_mcp_result_artifact', { reference: payload.result.artifact });
            if (epoch.current === generation) setArtifact(JSON.stringify(value, null, 2));
        } catch { if (epoch.current === generation) setError(true); }
        finally { if (epoch.current === generation) setBusy(false); }
    };
    const result = payload?.result;
    return <div className="my-2 rounded border border-(--border-default) p-3 text-xs text-(--fg-secondary)">
        <p className="break-all font-medium text-(--fg-primary)">{payload?.server && payload.tool ? `${payload.server} / ${payload.tool}` : `${t('mcpChat.tool')} · ${alias}`}</p>
        <p className="mt-1">{result ? t(`mcpChat.outcome.${result.outcome}`) : t(raw || status === 'error' ? 'mcpChat.invalidResult' : 'mcpChat.waiting')}</p>
        {result?.error && <p>{t(integrationProbeErrorKey(result.error))}</p>}
        {result?.projection?.is_error && <p>{t('mcpChat.toolError')}</p>}
        {result?.projection && <pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap break-all">{result.projection.text}</pre>}
        {result?.projection?.truncated && <p>{t('mcpChat.truncated')}</p>}
        {!!result?.projection?.non_text_blocks && <p>{t('mcpChat.nonText', { count: result.projection.non_text_blocks })}</p>}
        {result?.artifact && <button className="mt-2 rounded border border-(--border-default) px-2 py-1 disabled:opacity-50" disabled={busy} onClick={load}>{t('mcpChat.fullResult')}</button>}
        {(error || result?.artifact_unavailable) && <p role="alert">{t('mcpChat.artifactUnavailable')}</p>}
        {artifact && <details open className="mt-2"><summary>{t('mcpChat.fullResult')}</summary><pre className="max-h-96 overflow-auto whitespace-pre-wrap break-all">{artifact}</pre></details>}
    </div>;
}
