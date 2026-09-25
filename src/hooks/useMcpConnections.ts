import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { McpConnectionStatus } from '../types/integrations';

/** One sequential status poll for the entire Settings section. Catalog payloads
 * are fetched separately only when their revision changes. */
export function useMcpConnections(workspacePath?: string | null) {
    const [statuses, setStatuses] = useState<McpConnectionStatus[]>([]);
    const [phase, setPhase] = useState<'loading' | 'ready' | 'failed'>('loading');
    const epoch = useRef(0);
    useEffect(() => {
        let disposed = false;
        let timer: ReturnType<typeof setTimeout> | undefined;
        epoch.current += 1;
        setStatuses([]); setPhase('loading');
        if (!workspacePath) return;
        const poll = async () => {
            const started = epoch.current;
            try {
                const next = await invoke<McpConnectionStatus[]>('get_mcp_connections', { workspacePath });
                if (!disposed && started === epoch.current) {
                    setStatuses(previous => JSON.stringify(previous) === JSON.stringify(next) ? previous : next);
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
    return { statuses, phase, update };
}
