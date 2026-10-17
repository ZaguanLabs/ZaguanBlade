import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { McpConnectionStatus } from '../types/integrations';

/** One sequential status poll for the entire Settings section. Catalog payloads
 * are fetched separately only when their revision changes. `remembered` lists
 * the servers that connect automatically in this workspace. */
export function useMcpConnections(workspacePath?: string | null) {
    const [statuses, setStatuses] = useState<McpConnectionStatus[]>([]);
    const [remembered, setRememberedIds] = useState<string[]>([]);
    const [phase, setPhase] = useState<'loading' | 'ready' | 'failed'>('loading');
    const epoch = useRef(0);
    useEffect(() => {
        let disposed = false;
        let timer: ReturnType<typeof setTimeout> | undefined;
        epoch.current += 1;
        setStatuses([]); setRememberedIds([]); setPhase('loading');
        if (!workspacePath) return;
        const poll = async () => {
            const started = epoch.current;
            try {
                const [next, ids] = await Promise.all([
                    invoke<McpConnectionStatus[]>('get_mcp_connections', { workspacePath }),
                    invoke<string[]>('get_remembered_mcp_launches', { workspacePath }).catch(() => null),
                ]);
                if (!disposed && started === epoch.current) {
                    setStatuses(previous => JSON.stringify(previous) === JSON.stringify(next) ? previous : next);
                    if (ids) setRememberedIds(previous => JSON.stringify(previous) === JSON.stringify(ids) ? previous : ids);
                    setPhase('ready');
                }
            } catch {
                if (!disposed && started === epoch.current) setPhase('failed');
            }
            if (!disposed) timer = setTimeout(() => void poll(), 1000);
        };
        void poll();
        return () => { disposed = true; epoch.current += 1; clearTimeout(timer); };
    }, [workspacePath]);
    const update = useCallback((status: McpConnectionStatus) => {
        epoch.current += 1;
        setStatuses(previous => [...previous.filter(item => item.integration_id !== status.integration_id), status]);
    }, []);
    const setRemembered = useCallback((integrationId: string, value: boolean) => {
        epoch.current += 1;
        setRememberedIds(previous => [...previous.filter(id => id !== integrationId), ...(value ? [integrationId] : [])]);
    }, []);
    return { statuses, remembered, phase, update, setRemembered };
}
