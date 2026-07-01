import type { IndexHealthSnapshot } from '../types/blade';

function getPendingFileCount(indexHealth: IndexHealthSnapshot): number {
    return Math.max(
        indexHealth.queued_files,
        indexHealth.stale_files + indexHealth.missing_files,
    );
}

function formatIndexingProgress(indexHealth: IndexHealthSnapshot): string | null {
    const pendingFiles = getPendingFileCount(indexHealth);
    if (pendingFiles > 0) {
        const completedQueuedFiles = Math.max(0, pendingFiles - indexHealth.queued_files);
        return `${completedQueuedFiles}/${pendingFiles}`;
    }

    if (indexHealth.supported_files > 0) {
        return `${indexHealth.indexed_files}/${indexHealth.supported_files}`;
    }

    return null;
}

export function shouldShowIndexStatusCue(indexHealth: IndexHealthSnapshot | null): indexHealth is IndexHealthSnapshot {
    if (!indexHealth) {
        return false;
    }

    return indexHealth.status === 'checking' || indexHealth.status === 'indexing';
}

/** Minimal shape of i18next's `t`, so this util stays framework-light and testable. */
export type IndexStatusTranslate = (key: string, options?: Record<string, unknown>) => string;

export function formatIndexStatusLabel(
    indexHealth: IndexHealthSnapshot,
    t: IndexStatusTranslate,
): string {
    if (indexHealth.status === 'indexing') {
        const progress = formatIndexingProgress(indexHealth);
        const currentFile = indexHealth.current_file?.trim();
        if (currentFile) {
            return progress
                ? t('statusBar.index.indexingFile', { file: currentFile, progress })
                : t('statusBar.index.indexingFileOnly', { file: currentFile });
        }
        // No current file. Distinguish the post-scan FINALIZATION (relationship
        // resolution + write-to-disk, which can run for minutes on a large repo)
        // from active extraction, so the long tail stops looking like a hang.
        // The backend zeroes `queued_files` when the per-file passes finish (it
        // does NOT zero stale_files/missing_files, which retain their initial audit
        // values — so `getPendingFileCount` is the wrong signal here).
        if (indexHealth.queued_files === 0) {
            return t('statusBar.index.finalizing');
        }
        return progress
            ? t('statusBar.index.indexingSymbols', { progress })
            : t('statusBar.index.indexingSymbolsOnly');
    }

    if (indexHealth.status === 'checking') {
        return t('statusBar.index.checking');
    }

    if (indexHealth.status === 'stale') {
        const pendingFiles = getPendingFileCount(indexHealth);
        if (pendingFiles > 0) {
            return t('statusBar.index.pending', { count: pendingFiles });
        }
    }

    if (indexHealth.status === 'partial') {
        const pendingFiles = getPendingFileCount(indexHealth);
        if (pendingFiles > 0) {
            return t('statusBar.index.partial', { count: pendingFiles });
        }
    }

    if (indexHealth.status === 'error' && !indexHealth.message.trim()) {
        return t('statusBar.index.unavailable');
    }

    return indexHealth.message;
}

export function formatIndexStatusTitle(indexHealth: IndexHealthSnapshot): string {
    const parts = [
        indexHealth.message,
        `${indexHealth.indexed_files}/${indexHealth.supported_files} files indexed`,
        `${indexHealth.symbol_count} symbols`,
    ];
    if (indexHealth.current_file) {
        parts.push(`Current file: ${indexHealth.current_file}`);
    }
    if (indexHealth.queued_files > 0) {
        parts.push(`${indexHealth.queued_files} queued`);
    }
    if (indexHealth.orphaned_files > 0) {
        parts.push(`${indexHealth.orphaned_files} orphaned`);
    }
    return parts.join(' • ');
}
