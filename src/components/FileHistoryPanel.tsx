import React from 'react';
import { useFileHistory } from '../hooks/useFileHistory';
import { RotateCcw, Clock } from 'lucide-react';

interface FileHistoryPanelProps {
    activeFile: string | null;
}

export const FileHistoryPanel: React.FC<FileHistoryPanelProps> = ({ activeFile }) => {
    const { history, loading, revertToSnapshot } = useFileHistory(activeFile);

    const formatTime = (ts: number) => {
        return new Date(ts).toLocaleString(undefined, {
            hour: 'numeric',
            minute: 'numeric',
            second: 'numeric',
            day: 'numeric',
            month: 'short'
        });
    };

    if (!activeFile) {
        return (
            <div className="flex flex-col items-center justify-center h-full text-[var(--fg-tertiary)] p-4 text-center">
                <Clock className="w-8 h-8 opacity-20 mb-2" />
                <p className="text-sm">No file selected</p>
            </div>
        );
    }

    if (loading && history.length === 0) {
        return <div className="p-4 text-xs text-[var(--fg-tertiary)]">Loading history...</div>;
    }

    if (history.length === 0) {
        return (
            <div className="flex flex-col items-center justify-center h-full text-[var(--fg-tertiary)] p-4 text-center">
                <Clock className="w-8 h-8 opacity-20 mb-2" />
                <p className="text-sm">No history for this file</p>
            </div>
        );
    }

    return (
        <div className="flex flex-col h-full bg-[var(--bg-panel)] w-full">
            <div className="p-3 border-b border-[var(--border-subtle)] font-medium text-xs uppercase tracking-wider text-[var(--fg-secondary)] flex items-center gap-2 select-none">
                <Clock className="w-3 h-3" />
                <span>File History</span>
            </div>

            <div className="flex-1 overflow-y-auto p-2 space-y-2">
                {history.map((entry) => (
                    <div key={entry.id} className="group p-2 rounded bg-[var(--bg-surface)] border border-[var(--border-subtle)] hover:border-[var(--border-focus)] transition-all">
                        <div className="flex justify-between items-center mb-1">
                            <span className="text-xs font-medium text-[var(--fg-primary)] opacity-90">
                                {formatTime(entry.timestamp)}
                            </span>
                            <button
                                onClick={() => {
                                    if (confirm('Are you sure you want to revert to this version? This will overwrite the current file.')) {
                                        revertToSnapshot(entry.id);
                                    }
                                }}
                                title="Revert to this version"
                                className="opacity-0 group-hover:opacity-100 p-1.5 hover:bg-[var(--bg-app)] rounded transition-opacity text-[var(--fg-link)] hover:text-amber-400"
                            >
                                <RotateCcw className="w-3.5 h-3.5" />
                            </button>
                        </div>
                        <div className="text-[10px] font-mono text-[var(--fg-tertiary)] truncate" title={entry.id}>
                            ID: {entry.id.substring(0, 8)}...
                        </div>
                    </div>
                ))}
            </div>
        </div>
    );
};
