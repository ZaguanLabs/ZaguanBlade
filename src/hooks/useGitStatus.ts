import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface GitStatusSummary {
    isRepo: boolean;
    changedCount: number;
    stagedCount: number;
    unstagedCount: number;
    untrackedCount: number;
    branch: string | null;
    ahead: number;
    behind: number;
    dirty: boolean;
}

export interface GitFileStatus {
    path: string;
    displayPath?: string | null;
    staged: boolean;
    unstaged: boolean;
    untracked: boolean;
    conflicted: boolean;
    statusCode: string;
}

const DEFAULT_DEBOUNCE_MS = 700;

export const useGitStatus = () => {
    const [status, setStatus] = useState<GitStatusSummary | null>(null);
    const [files, setFiles] = useState<GitFileStatus[]>([]);
    const [error, setError] = useState<string | null>(null);
    const [filesError, setFilesError] = useState<string | null>(null);
    const [lastRefreshedAt, setLastRefreshedAt] = useState<number | null>(null);
    const debounceRef = useRef<number | null>(null);

    const refresh = useCallback(async () => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;

        const [summaryResult, filesResult] = await Promise.allSettled([
            invoke<GitStatusSummary>('git_status_summary'),
            invoke<GitFileStatus[]>('git_status_files'),
        ]);

        if (summaryResult.status === 'fulfilled') {
            setStatus(summaryResult.value);
            setError(null);
        } else {
            setError(String(summaryResult.reason));
        }

        if (filesResult.status === 'fulfilled') {
            setFiles(filesResult.value);
            setFilesError(null);
        } else {
            setFilesError(String(filesResult.reason));
        }

        setLastRefreshedAt(Date.now());
    }, []);

    const scheduleRefresh = useCallback(() => {
        if (debounceRef.current) {
            window.clearTimeout(debounceRef.current);
        }
        debounceRef.current = window.setTimeout(() => {
            refresh();
        }, DEFAULT_DEBOUNCE_MS);
    }, [refresh]);

    const stageFile = useCallback(async (path: string) => {
        await invoke('git_stage_file', { path });
        await refresh();
    }, [refresh]);

    const unstageFile = useCallback(async (path: string) => {
        await invoke('git_unstage_file', { path });
        await refresh();
    }, [refresh]);

    const stageAll = useCallback(async () => {
        await invoke('git_stage_all');
        await refresh();
    }, [refresh]);

    const unstageAll = useCallback(async () => {
        await invoke('git_unstage_all');
        await refresh();
    }, [refresh]);

    const commit = useCallback(async (message: string) => {
        await invoke('git_commit', { message });
        await refresh();
    }, [refresh]);

    const push = useCallback(async () => {
        await invoke('git_push');
        await refresh();
    }, [refresh]);

    const diff = useCallback(async (path: string, staged: boolean) => {
        return invoke<string>('git_diff', { path, staged });
    }, []);

    const generateCommitMessage = useCallback(async (modelId?: string) => {
        if (modelId) {
            return await invoke<string>('git_generate_commit_message_ai', { modelId });
        }
        return invoke<string>('git_generate_commit_message');
    }, []);

    useEffect(() => {
        refresh();

        let unlisten: (() => void) | undefined;
        const setupListener = async () => {
            unlisten = await listen('file-changes-detected', () => {
                scheduleRefresh();
            });
        };
        setupListener();

        const onVisibilityChange = () => {
            if (document.visibilityState === 'visible') {
                scheduleRefresh();
            }
        };

        const onFocus = () => {
            scheduleRefresh();
        };

        document.addEventListener('visibilitychange', onVisibilityChange);
        window.addEventListener('focus', onFocus);

        return () => {
            if (unlisten) unlisten();
            document.removeEventListener('visibilitychange', onVisibilityChange);
            window.removeEventListener('focus', onFocus);
            if (debounceRef.current) {
                window.clearTimeout(debounceRef.current);
            }
        };
    }, [refresh, scheduleRefresh]);

    return {
        status,
        files,
        error,
        filesError,
        lastRefreshedAt,
        refresh,
        scheduleRefresh,
        stageFile,
        unstageFile,
        stageAll,
        unstageAll,
        commit,
        push,
        diff,
        generateCommitMessage,
    };
};
