import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { formatUnknownBackendError } from '../utils/backendErrors';

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

interface GitStatusSnapshot {
    summary: GitStatusSummary;
    files: GitFileStatus[];
}

export interface CommitPreflightResult {
    canCommit: boolean;
    isRepo: boolean;
    branch: string | null;
    isDetached: boolean;
    hasUpstream: boolean;
    hasConflicts: boolean;
    stagedCount: number;
    errorMessage: string | null;
    errorKey?: string | null;
}

const DEFAULT_DEBOUNCE_MS = 700;

interface UseGitStatusOptions {
    includeFiles?: boolean;
}

export const useGitStatus = ({ includeFiles = false }: UseGitStatusOptions = {}) => {
    const [status, setStatus] = useState<GitStatusSummary | null>(null);
    const [files, setFiles] = useState<GitFileStatus[]>([]);
    const [error, setError] = useState<string | null>(null);
    const [filesError, setFilesError] = useState<string | null>(null);
    const [lastRefreshedAt, setLastRefreshedAt] = useState<number | null>(null);
    const debounceRef = useRef<number | null>(null);
    const refreshInFlightRef = useRef(false);
    const refreshFilesInFlightRef = useRef(false);

    const refreshSummary = useCallback(async () => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
        if (refreshInFlightRef.current) return;
        refreshInFlightRef.current = true;

        try {
            const summary = await invoke<GitStatusSummary>('git_status_summary');
            setStatus(summary);
            setError(null);
        } catch (err) {
            setError(formatUnknownBackendError(err));
        } finally {
            refreshInFlightRef.current = false;
            setLastRefreshedAt(Date.now());
        }
    }, []);

    const refreshFiles = useCallback(async () => {
        if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
        if (refreshFilesInFlightRef.current) return;
        refreshFilesInFlightRef.current = true;

        try {
            const nextFiles = await invoke<GitFileStatus[]>('git_status_files');
            setFiles(nextFiles);
            setFilesError(null);
        } catch (err) {
            setFilesError(formatUnknownBackendError(err));
        } finally {
            refreshFilesInFlightRef.current = false;
            setLastRefreshedAt(Date.now());
        }
    }, []);

    const refresh = useCallback(async () => {
        await refreshSummary();

        if (includeFiles) {
            await refreshFiles();
        }
    }, [includeFiles, refreshFiles, refreshSummary]);

    const scheduleRefresh = useCallback(() => {
        if (debounceRef.current) {
            window.clearTimeout(debounceRef.current);
        }
        debounceRef.current = window.setTimeout(() => {
            if (includeFiles) {
                void refresh();
                return;
            }

            void refreshSummary();
        }, DEFAULT_DEBOUNCE_MS);
    }, [includeFiles, refresh, refreshSummary]);

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
        // Align with push intent: if user is pushing, treat pending AI edits as accepted.
        await invoke('accept_all_changes');
        window.dispatchEvent(new CustomEvent('uncommitted-changes-updated'));
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

    const commitPreflight = useCallback(async () => {
        return invoke<CommitPreflightResult>('git_commit_preflight');
    }, []);

    useEffect(() => {
        void refreshSummary();

        if (includeFiles) {
            void refreshFiles();
        }

        const unlistenPromise = listen('file-changes-detected', () => {
            scheduleRefresh();
        });

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
            unlistenPromise
                .then(unlisten => unlisten())
                .catch(console.error);
            document.removeEventListener('visibilitychange', onVisibilityChange);
            window.removeEventListener('focus', onFocus);
            if (debounceRef.current) {
                window.clearTimeout(debounceRef.current);
            }
        };
    }, [includeFiles, refreshFiles, refreshSummary, scheduleRefresh]);

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
        commitPreflight,
    };
};
