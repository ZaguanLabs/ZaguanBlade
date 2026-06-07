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

    if (indexHealth.status === 'checking' || indexHealth.status === 'indexing' || indexHealth.status === 'error') {
        return true;
    }

    return (indexHealth.status === 'partial' || indexHealth.status === 'stale')
        && (getPendingFileCount(indexHealth) > 0 || indexHealth.orphaned_files > 0);
}

export function formatIndexStatusLabel(indexHealth: IndexHealthSnapshot): string {
    if (indexHealth.status === 'indexing') {
        const progress = formatIndexingProgress(indexHealth);
        const currentFile = indexHealth.current_file?.trim();
        if (currentFile) {
            return progress
                ? `Indexing ${currentFile} (${progress})`
                : `Indexing ${currentFile}`;
        }
        return progress ? `Indexing symbols (${progress})` : 'Indexing symbols';
    }

    if (indexHealth.status === 'checking') {
        return 'Checking code index';
    }

    if (indexHealth.status === 'stale') {
        const pendingFiles = getPendingFileCount(indexHealth);
        if (pendingFiles > 0) {
            return `Symbol index pending (${pendingFiles} files)`;
        }
    }

    if (indexHealth.status === 'partial') {
        const pendingFiles = getPendingFileCount(indexHealth);
        if (pendingFiles > 0) {
            return `Symbol index partial (${pendingFiles} files pending)`;
        }
    }

    if (indexHealth.status === 'error' && !indexHealth.message.trim()) {
        return 'Symbol index unavailable';
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
