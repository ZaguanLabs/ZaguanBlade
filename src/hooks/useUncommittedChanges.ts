import { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UncommittedChange } from '../types/uncommitted';
import {
  createUncommittedChangesUpdatedEvent,
  notifyUncommittedFilesChanged,
  type UncommittedChangesUpdatedDetail,
  type UncommittedChangesUpdateReason,
} from '../utils/uncommittedChangeNotifications';
import {
  normalizeUncommittedPath,
  upsertUncommittedChangeState,
} from '../utils/uncommittedChangesState';

interface UseUncommittedChangesOptions {
  onFileChanged?: (filePath: string, reason: UncommittedChangesUpdateReason) => void;
  trackChanges?: boolean;
}

export function useUncommittedChanges(options?: UseUncommittedChangesOptions) {
  const trackChanges = options?.trackChanges ?? true;
  const [changes, setChanges] = useState<UncommittedChange[]>([]);
  const [loading, setLoading] = useState(trackChanges);
  const sourceIdRef = useRef(`uncommitted-${crypto.randomUUID()}`);
  const onFileChangedRef = useRef(options?.onFileChanged);

  useEffect(() => {
    onFileChangedRef.current = options?.onFileChanged;
  }, [options?.onFileChanged]);

  const normalizePath = useCallback((value: string): string => {
    return normalizeUncommittedPath(value);
  }, []);

  const isBoundarySuffixMatch = useCallback((full: string, suffix: string): boolean => {
    if (!full.endsWith(suffix)) return false;
    if (full.length === suffix.length) return true;
    return full[full.length - suffix.length - 1] === '/';
  }, []);

  const refresh = useCallback(async () => {
    if (!trackChanges) {
      setLoading(false);
      return;
    }

    try {
      const result = await invoke<UncommittedChange[]>('get_uncommitted_changes');
      setChanges(result.filter(change => change.unified_diff.trim().length > 0));
    } catch (error) {
      console.error('Failed to get uncommitted changes:', error);
    } finally {
      setLoading(false);
    }
  }, [trackChanges]);

  const upsertChange = useCallback((filePath: string, nextChange: UncommittedChange | null) => {
    setChanges(prev => {
      return upsertUncommittedChangeState(prev, filePath, nextChange);
    });
  }, []);

  const removeChanges = useCallback((predicate: (change: UncommittedChange) => boolean) => {
    setChanges(prev => prev.filter(change => !predicate(change)));
  }, []);

  const notifyUpdated = useCallback((filePaths?: string[], reason: UncommittedChangesUpdateReason = 'refresh') => {
    window.dispatchEvent(createUncommittedChangesUpdatedEvent(sourceIdRef.current, filePaths, reason));
  }, []);

  const notifyLocalFileChanged = useCallback((filePaths: string[], reason: UncommittedChangesUpdateReason) => {
    notifyUncommittedFilesChanged(filePaths, reason, onFileChangedRef.current);
  }, []);

  useEffect(() => {
    if (trackChanges) {
      void refresh();
    } else {
      setLoading(false);
    }

    const unlistenPromise = listen<{ change_id: string; file_path: string; file_paths?: string[] }>('change-applied', (event) => {
      void (async () => {
        const affectedPaths = event.payload.file_paths?.length
          ? event.payload.file_paths
          : [event.payload.file_path];

        if (trackChanges) {
          try {
            const nextChanges = await Promise.all(
              affectedPaths.map(filePath => invoke<UncommittedChange | null>('get_uncommitted_change_for_file', {
                filePath,
              }))
            );
            nextChanges.forEach((nextChange, index) => {
              const affectedPath = affectedPaths[index] ?? nextChange?.file_path;
              if (affectedPath) {
                upsertChange(affectedPath, nextChange);
              }
            });
          } catch (error) {
            console.error('Failed to get uncommitted change for file:', error);
            void refresh();
          }
        }

        if (onFileChangedRef.current) {
          affectedPaths.forEach(filePath => onFileChangedRef.current?.(filePath, 'applied'));
        }
      })();
    });

    // Listen for cross-instance refresh events
    const handleGlobalRefresh = (event: Event) => {
      const customEvent = event as CustomEvent<UncommittedChangesUpdatedDetail>;
      if (customEvent.detail?.sourceId === sourceIdRef.current) {
        return;
      }
      if (trackChanges) {
        void refresh();
      }

      if (onFileChangedRef.current) {
        const reason = customEvent.detail?.reason ?? 'refresh';
        customEvent.detail?.filePaths?.forEach(filePath => onFileChangedRef.current?.(filePath, reason));
      }
    };
    window.addEventListener('uncommitted-changes-updated', handleGlobalRefresh as EventListener);

    return () => {
      unlistenPromise
        .then(unlisten => unlisten())
        .catch(console.error);
      window.removeEventListener('uncommitted-changes-updated', handleGlobalRefresh as EventListener);
    };
  }, [refresh, trackChanges, upsertChange]);

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
      const removed = await invoke<UncommittedChange>('accept_change', { id });
      removeChanges(change => change.id === removed.id);
      notifyLocalFileChanged([removed.file_path], 'accepted');
      notifyUpdated([removed.file_path], 'accepted');
      return true;
    } catch (error) {
      console.error('Failed to accept change:', error);
      return false;
    }
  }, [notifyLocalFileChanged, notifyUpdated, removeChanges]);

  const acceptFile = useCallback(async (filePath: string): Promise<boolean> => {
    try {
      const removed = await invoke<UncommittedChange>('accept_file_changes', { filePath });
      const removedPath = normalizePath(removed.file_path);
      removeChanges(change => normalizePath(change.file_path) === removedPath);
      notifyLocalFileChanged([removed.file_path], 'accepted');
      notifyUpdated([removed.file_path], 'accepted');
      return true;
    } catch (error) {
      console.error('Failed to accept file changes:', error);
      return false;
    }
  }, [normalizePath, notifyLocalFileChanged, notifyUpdated, removeChanges]);

  const acceptAll = useCallback(async (): Promise<boolean> => {
    try {
      const removed = await invoke<UncommittedChange[]>('accept_all_changes');
      setChanges([]);
      notifyLocalFileChanged(removed.map(change => change.file_path), 'accepted');
      notifyUpdated(removed.map(change => change.file_path), 'accepted');
      return true;
    } catch (error) {
      console.error('Failed to accept all changes:', error);
      return false;
    }
  }, [notifyLocalFileChanged, notifyUpdated]);

  const rejectChange = useCallback(async (id: string): Promise<boolean> => {
    try {
      const removed = await invoke<UncommittedChange>('reject_change', { id });
      removeChanges(change => change.id === removed.id);
      notifyLocalFileChanged([removed.file_path], 'rejected');
      notifyUpdated([removed.file_path], 'rejected');
      return true;
    } catch (error) {
      console.error('Failed to reject change:', error);
      return false;
    }
  }, [notifyLocalFileChanged, notifyUpdated, removeChanges]);

  const rejectFile = useCallback(async (filePath: string): Promise<boolean> => {
    try {
      const removed = await invoke<UncommittedChange>('reject_file_changes', { filePath });
      const removedPath = normalizePath(removed.file_path);
      removeChanges(change => normalizePath(change.file_path) === removedPath);
      notifyLocalFileChanged([removed.file_path], 'rejected');
      notifyUpdated([removed.file_path], 'rejected');
      return true;
    } catch (error) {
      console.error('Failed to reject file changes:', error);
      return false;
    }
  }, [normalizePath, notifyLocalFileChanged, notifyUpdated, removeChanges]);

  const rejectAll = useCallback(async (): Promise<boolean> => {
    try {
      const removed = await invoke<UncommittedChange[]>('reject_all_changes');
      setChanges([]);
      notifyLocalFileChanged(removed.map(change => change.file_path), 'rejected');
      notifyUpdated(removed.map(change => change.file_path), 'rejected');
      return true;
    } catch (error) {
      console.error('Failed to reject all changes:', error);
      return false;
    }
  }, [notifyLocalFileChanged, notifyUpdated]);

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
