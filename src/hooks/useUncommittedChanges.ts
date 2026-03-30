import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UncommittedChange } from '../types/uncommitted';

interface UseUncommittedChangesOptions {
  onFileChanged?: (filePath: string) => void;
}

interface UncommittedChangesUpdatedDetail {
  sourceId?: string;
}

export function useUncommittedChanges(options?: UseUncommittedChangesOptions) {
  const [changes, setChanges] = useState<UncommittedChange[]>([]);
  const [loading, setLoading] = useState(true);
  const sourceIdRef = useRef(`uncommitted-${crypto.randomUUID()}`);

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

  const upsertChange = useCallback((nextChange: UncommittedChange | null) => {
    setChanges(prev => {
      if (!nextChange) {
        return prev;
      }

      const nextPath = normalizePath(nextChange.file_path);
      const filtered = prev.filter(change => normalizePath(change.file_path) !== nextPath);
      return [...filtered, nextChange].sort((a, b) => a.timestamp - b.timestamp);
    });
  }, [normalizePath]);

  const removeChanges = useCallback((predicate: (change: UncommittedChange) => boolean) => {
    setChanges(prev => prev.filter(change => !predicate(change)));
  }, []);

  const notifyUpdated = useCallback(() => {
    window.dispatchEvent(new CustomEvent<UncommittedChangesUpdatedDetail>('uncommitted-changes-updated', {
      detail: { sourceId: sourceIdRef.current },
    }));
  }, []);

  useEffect(() => {
    refresh();

    const unlistenPromise = listen<{ change_id: string; file_path: string; file_paths?: string[] }>('change-applied', (event) => {
      void (async () => {
        const affectedPaths = event.payload.file_paths?.length
          ? event.payload.file_paths
          : [event.payload.file_path];

        try {
          const nextChanges = await Promise.all(
            affectedPaths.map(filePath => invoke<UncommittedChange | null>('get_uncommitted_change_for_file', {
              filePath,
            }))
          );
          nextChanges.forEach(upsertChange);
        } catch (error) {
          console.error('Failed to get uncommitted change for file:', error);
          void refresh();
        }

        if (options?.onFileChanged) {
          affectedPaths.forEach(filePath => options.onFileChanged?.(filePath));
        }
      })();
    });

    // Listen for cross-instance refresh events
    const handleGlobalRefresh = (event: Event) => {
      const customEvent = event as CustomEvent<UncommittedChangesUpdatedDetail>;
      if (customEvent.detail?.sourceId === sourceIdRef.current) {
        return;
      }
      void refresh();
    };
    window.addEventListener('uncommitted-changes-updated', handleGlobalRefresh as EventListener);

    return () => {
      unlistenPromise
        .then(unlisten => unlisten())
        .catch(console.error);
      window.removeEventListener('uncommitted-changes-updated', handleGlobalRefresh as EventListener);
    };
  }, [refresh, options?.onFileChanged, upsertChange]);

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
      await invoke<UncommittedChange>('accept_change', { id });
      removeChanges(change => change.id === id);
      notifyUpdated();
      return true;
    } catch (error) {
      console.error('Failed to accept change:', error);
      return false;
    }
  }, [notifyUpdated, removeChanges]);

  const acceptFile = useCallback(async (filePath: string): Promise<boolean> => {
    try {
      const removed = await invoke<UncommittedChange>('accept_file_changes', { filePath });
      const removedPath = normalizePath(removed.file_path);
      removeChanges(change => normalizePath(change.file_path) === removedPath);
      notifyUpdated();
      return true;
    } catch (error) {
      console.error('Failed to accept file changes:', error);
      return false;
    }
  }, [normalizePath, notifyUpdated, removeChanges]);

  const acceptAll = useCallback(async (): Promise<boolean> => {
    try {
      await invoke<UncommittedChange[]>('accept_all_changes');
      setChanges([]);
      notifyUpdated();
      return true;
    } catch (error) {
      console.error('Failed to accept all changes:', error);
      return false;
    }
  }, [notifyUpdated]);

  const rejectChange = useCallback(async (id: string): Promise<boolean> => {
    try {
      await invoke<UncommittedChange>('reject_change', { id });
      removeChanges(change => change.id === id);
      notifyUpdated();
      return true;
    } catch (error) {
      console.error('Failed to reject change:', error);
      return false;
    }
  }, [notifyUpdated, removeChanges]);

  const rejectFile = useCallback(async (filePath: string): Promise<boolean> => {
    try {
      const removed = await invoke<UncommittedChange>('reject_file_changes', { filePath });
      const removedPath = normalizePath(removed.file_path);
      removeChanges(change => normalizePath(change.file_path) === removedPath);
      notifyUpdated();
      return true;
    } catch (error) {
      console.error('Failed to reject file changes:', error);
      return false;
    }
  }, [normalizePath, notifyUpdated, removeChanges]);

  const rejectAll = useCallback(async (): Promise<boolean> => {
    try {
      await invoke<UncommittedChange[]>('reject_all_changes');
      setChanges([]);
      notifyUpdated();
      return true;
    } catch (error) {
      console.error('Failed to reject all changes:', error);
      return false;
    }
  }, [notifyUpdated]);

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
