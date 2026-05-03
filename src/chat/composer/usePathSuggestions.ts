import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface WorkspacePathMatch {
    path: string;
    is_dir: boolean;
}

export function usePathSuggestions(query: string, enabled: boolean): WorkspacePathMatch[] {
    const [suggestions, setSuggestions] = useState<WorkspacePathMatch[]>([]);

    useEffect(() => {
        if (!enabled) {
            setSuggestions([]);
            return;
        }

        let cancelled = false;
        const timer = window.setTimeout(async () => {
            try {
                const results = await invoke<WorkspacePathMatch[]>('search_workspace_paths', {
                    query,
                    limit: 12,
                });
                if (!cancelled) {
                    setSuggestions(results);
                }
            } catch (error) {
                if (!cancelled) {
                    console.error('[Composer] Failed to search workspace paths:', error);
                    setSuggestions([]);
                }
            }
        }, 120);

        return () => {
            cancelled = true;
            window.clearTimeout(timer);
        };
    }, [enabled, query]);

    return suggestions;
}
