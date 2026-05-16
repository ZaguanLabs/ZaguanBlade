import React from 'react';
import { useTranslation } from 'react-i18next';
import { useFileHistory } from '../hooks/useFileHistory';
import { RotateCcw, Clock } from 'lucide-react';
import { ScrollArea } from './ui/ScrollArea';

interface FileHistoryPanelProps {
    activeFile: string | null;
}

export const FileHistoryPanel: React.FC<FileHistoryPanelProps> = ({ activeFile }) => {
    const { t } = useTranslation();
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
            <div className="flex flex-col items-center justify-center h-full text-(--fg-tertiary) p-4 text-center">
                <Clock className="w-8 h-8 opacity-20 mb-2" />
                <p className="text-sm">{t('fileHistory.noFileSelected')}</p>
            </div>
        );
    }

    if (loading && history.length === 0) {
        return <div className="p-4 text-xs text-(--fg-tertiary)">{t('fileHistory.loading')}</div>;
    }

    if (history.length === 0) {
        return (
            <div className="flex flex-col items-center justify-center h-full text-(--fg-tertiary) p-4 text-center">
                <Clock className="w-8 h-8 opacity-20 mb-2" />
                <p className="text-sm">{t('fileHistory.empty')}</p>
            </div>
        );
    }

    return (
        <div className="flex flex-col h-full bg-(--bg-panel) w-full">
            <div className="p-3 border-b border-(--border-subtle) font-medium text-xs uppercase tracking-wider text-(--fg-secondary) flex items-center gap-2 select-none">
                <Clock className="w-3 h-3" />
                <span>{t('fileHistory.title')}</span>
            </div>

            <ScrollArea className="flex-1 p-2 space-y-2">
                {history.map((entry) => (
                    <div key={entry.id} className="group p-2 rounded-[calc(var(--panel-radius)*0.65)] bg-(--bg-surface) border border-(--border-subtle) hover:border-(--border-focus) transition-all">
                        <div className="flex justify-between items-center mb-1">
                            <span className="text-xs font-medium text-(--fg-primary) opacity-90">
                                {formatTime(entry.timestamp)}
                            </span>
                            <button
                                onClick={() => {
                                    if (confirm(t('fileHistory.confirmRevert'))) {
                                        revertToSnapshot(entry.id);
                                    }
                                }}
                                title={t('fileHistory.revertToVersion')}
                                className="opacity-0 group-hover:opacity-100 p-1.5 hover:bg-(--bg-app) rounded-[calc(var(--panel-radius)*0.45)] transition-opacity text-(--accent-ai) hover:text-(--accent-warning)"
                            >
                                <RotateCcw className="w-3.5 h-3.5" />
                            </button>
                        </div>
                        <div className="text-[10px] font-mono text-(--fg-tertiary) truncate" title={entry.id}>
                            {t('fileHistory.snapshotId', { id: entry.id.substring(0, 8) })}
                        </div>
                    </div>
                ))}
            </ScrollArea>
        </div>
    );
};
