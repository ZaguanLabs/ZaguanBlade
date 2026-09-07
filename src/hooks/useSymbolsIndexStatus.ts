import { useEffect, useState, useSyncExternalStore } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { IndexHealthSnapshot } from '../types/blade';

export interface SymbolsIndexStatus {
    workspace: { workspace_id: string; generation: string };
    enabled: boolean;
    health: IndexHealthSnapshot;
}
let available = false;
const listeners = new Set<() => void>();
function publish(value: boolean) {
    if (available === value) return;
    available = value;
    for (const listener of listeners) listener();
}
function subscribe(listener: () => void) {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
}
export function useSymbolsIndexAvailable(): boolean {
    return useSyncExternalStore(subscribe, () => available, () => false);
}

/** Polls sequentially. Workspace changes invalidate both late responses and the
 * inspector immediately. Reading status never initializes the symbol database. */
export function useSymbolsIndexStatus(workspacePath?: string | null, publishAvailability = false) {
    const [status, setStatus] = useState<SymbolsIndexStatus | null>(null);
    const [failed, setFailed] = useState(false);
    useEffect(() => {
        let disposed = false;
        let timer: ReturnType<typeof setTimeout> | undefined;
        setStatus(null); setFailed(false);
        if (publishAvailability) publish(false);
        if (!workspacePath) return;
        const poll = async () => {
            try {
                const next = await invoke<SymbolsIndexStatus>('get_symbols_index_status', { workspacePath });
                if (disposed) return;
                setStatus(next); setFailed(false);
                if (publishAvailability) publish(next.enabled && next.health.status !== 'stopping');
            } catch {
                if (disposed) return;
                setStatus(null); setFailed(true);
                if (publishAvailability) publish(false);
            }
            if (!disposed) timer = setTimeout(() => void poll(), 1000);
        };
        void poll();
        return () => { disposed = true; clearTimeout(timer); if (publishAvailability) publish(false); };
    }, [workspacePath, publishAvailability]);
    return { status, failed };
}
