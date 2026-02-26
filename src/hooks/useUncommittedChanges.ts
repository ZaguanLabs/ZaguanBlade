import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UncommittedChange } from '../types/uncommitted';

interface UseUncommittedChangesOptions {
  onFileChanged?: (filePath: string) => void;
}

export function useUncommittedChanges(options?: UseUncommittedChangesOptions) {
  const [changes, setChanges] = useState<UncommittedChange[]>([]);
  const [loading, setLoading] = useState(true);

  const normalizePath = useCallback((value: string): string => {
    return value.replace(/\\/g, '/').replace(/\/+/g, '/').replace(/\/$/, '');
  }, []);

  const isBoundarySuffixMatch = useCallback((full: string, suffix: string): boolean => {
    if (!full.endsWith(suffix)) return false;
    if (full.length === suffix.length) return true;
    return full[full.length - suffix.length - 1] === '/';
  }, []);

  const refresh = useCallback(async () => {
    try {
      const result = await invoke<UncommittedChange[]>('get_uncommitted_changes');
      setChanges(result);
    } catch (error) {
      console.error('Failed to get uncommitted changes:', error);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();

    const unlistenPromise = listen<{ change_id: string; file_path: string }>('change-applied', (event) => {
      refresh();
      if (options?.onFileChanged && event.payload?.file_path) {
        options.onFileChanged(event.payload.file_path);
      }
    });

    // Listen for cross-instance refresh events
    const handleGlobalRefresh = () => refresh();
    window.addEventListener('uncommitted-changes-updated', handleGlobalRefresh);

    return () => {
      unlistenPromise
        .then(unlisten => unlisten())
        .catch(console.error);
      window.removeEventListener('uncommitted-changes-updated', handleGlobalRefresh);
    };
  }, [refresh, options?.onFileChanged]);

  const getChangeForFile = useCallback((filePath: string): UncommittedChange | undefined => {
    const target = normalizePath(filePath);

    // Primary matching: normalized exact path.
    let matches = changes.filter(c => normalizePath(c.file_path) === target);

    // Fallback only when no exact match is available: boundary-safe suffix matching
    // for absolute/relative path representation differences.
    if (matches.length === 0) {
      matches = changes.filter(c => {
        const candidate = normalizePath(c.file_path);
        return isBoundarySuffixMatch(candidate, target) || isBoundarySuffixMatch(target, candidate);
      });
    }

    if (matches.length === 0) return undefined;

    // Deterministic latest-first selection.
    const sorted = [...matches].sort((a, b) => b.timestamp - a.timestamp);

    // Prefer the newest non-empty diff.
    for (const change of sorted) {
      if (change.unified_diff.trim().length > 0) {
        return change;
      }
    }

    return sorted[0];
  }, [changes, normalizePath, isBoundarySuffixMatch]);

  const acceptChange = useCallback(async (id: string): Promise<boolean> => {
    try {
      await invoke('accept_change', { id });
      await refresh();
      window.dispatchEvent(new CustomEvent('uncommitted-changes-updated'));
      return true;
    } catch (error) {
      console.error('Failed to accept change:', error);
      return false;
    }
  }, [refresh]);

  const acceptFile = useCallback(async (filePath: string): Promise<boolean> => {
    try {
      await invoke('accept_file_changes', { filePath });
      await refresh();
      window.dispatchEvent(new CustomEvent('uncommitted-changes-updated'));
      return true;
    } catch (error) {
      console.error('Failed to accept file changes:', error);
      return false;
    }
  }, [refresh]);

  const acceptAll = useCallback(async (): Promise<boolean> => {
    try {
      await invoke('accept_all_changes');
      await refresh();
      // Emit event so other hook instances can refresh
      window.dispatchEvent(new CustomEvent('uncommitted-changes-updated'));
      return true;
    } catch (error) {
      console.error('Failed to accept all changes:', error);
      return false;
    }
  }, [refresh]);

  const rejectChange = useCallback(async (id: string): Promise<boolean> => {
    try {
      await invoke('reject_change', { id });
      await refresh();
      window.dispatchEvent(new CustomEvent('uncommitted-changes-updated'));
      return true;
    } catch (error) {
      console.error('Failed to reject change:', error);
      return false;
    }
  }, [refresh]);

  const rejectFile = useCallback(async (filePath: string): Promise<boolean> => {
    try {
      await invoke('reject_file_changes', { filePath });
      await refresh();
      window.dispatchEvent(new CustomEvent('uncommitted-changes-updated'));
      return true;
    } catch (error) {
      console.error('Failed to reject file changes:', error);
      return false;
    }
  }, [refresh]);

  const rejectAll = useCallback(async (): Promise<boolean> => {
    try {
      await invoke('reject_all_changes');
      await refresh();
      // Emit event so other hook instances can refresh
      window.dispatchEvent(new CustomEvent('uncommitted-changes-updated'));
      return true;
    } catch (error) {
      console.error('Failed to reject all changes:', error);
      return false;
    }
  }, [refresh]);

  return {
    changes,
    loading,
    refresh,
    getChangeForFile,
    acceptChange,
    acceptFile,
    acceptAll,
    rejectChange,
    rejectFile,
    rejectAll,
    count: changes.length,
  };
}
