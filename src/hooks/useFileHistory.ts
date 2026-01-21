import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface HistoryEntry {
    id: string;
    file_path: string;
    timestamp: number;
    snapshot_path: string;
}

export function useFileHistory(filePath: string | null) {
    const [history, setHistory] = useState<HistoryEntry[]>([]);
    const [loading, setLoading] = useState(false);

    const fetchHistory = useCallback(async () => {
        if (!filePath) {
            setHistory([]);
            return;
        }
        setLoading(true);
        try {
            const entries = await invoke<HistoryEntry[]>('get_file_history', { path: filePath });
            // Sort descending by timestamp
            entries.sort((a, b) => b.timestamp - a.timestamp);
            setHistory(entries);
        } catch (e) {
            console.error('Failed to fetch history:', e);
        } finally {
            setLoading(false);
        }
    }, [filePath]);

    useEffect(() => {
        fetchHistory();
    }, [fetchHistory]);

    // Listen to history-entry-added
    useEffect(() => {
        let unlisten: (() => void) | undefined;

        const setupListener = async () => {
            unlisten = await listen<{ entry: HistoryEntry }>('history-entry-added', (event) => {
                // Check if the added entry matches current file
                // Backend sends absolute path. Ensure string comparison works.
                if (filePath && event.payload.entry.file_path === filePath) {
                    setHistory(prev => {
                        const newEntry = event.payload.entry;
                        const newList = [newEntry, ...prev];
                        return newList.sort((a, b) => b.timestamp - a.timestamp);
                    });
                }
            });
        };

        setupListener();

        return () => {
            if (unlisten) unlisten();
        };
    }, [filePath]);

    const revertToSnapshot = async (snapshotId: string) => {
        try {
            await invoke('revert_file_to_snapshot', { snapshotId });
        } catch (e) {
            console.error('Failed to revert:', e);
            throw e;
        }
    };

    return { history, loading, revertToSnapshot, refresh: fetchHistory };
}
