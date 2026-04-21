import React from 'react';
import { useTranslation } from 'react-i18next';
import { ListOrdered, Pencil, Trash2 } from 'lucide-react';
import type { QueuedRequest } from '../types/chat';

interface QueuePanelProps {
    requests: QueuedRequest[];
    onEditRequest: (index: number) => void;
    onDeleteRequest: (index: number) => void;
}

const QueuePanelComponent: React.FC<QueuePanelProps> = ({ requests, onEditRequest, onDeleteRequest }) => {
    const { t } = useTranslation();
    if (requests.length === 0) return null;

    return (
        <div className="border-t border-(--border-subtle) bg-(--bg-surface)">
            <div className="flex items-center gap-3 px-3 py-2.5 text-[11px] text-(--fg-secondary)">
                <div className="flex h-7 w-7 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-950/40">
                    <ListOrdered className="h-3.5 w-3.5 text-zinc-400" />
                </div>
                <span className="font-semibold uppercase tracking-[0.16em]">{t('chat.queue.title', { count: requests.length })}</span>
            </div>
            <div className="max-h-[200px] space-y-1 overflow-auto px-3 pb-3">
                {requests.map((request, index) => {
                    const preview = request.text.trim() || t('chat.queue.imageOnlyRequest');
                    return (
                        <div
                            key={`queued-${index}`}
                            className="flex items-center gap-2 rounded-xl border border-zinc-800/70 bg-zinc-950/30 px-2.5 py-2 text-[11px] text-zinc-400"
                        >
                            <span className="font-mono text-[10px] text-zinc-600 w-4 text-right shrink-0">
                                {index + 1}.
                            </span>
                            <span className={`inline-flex shrink-0 items-center rounded-md border px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.12em] ${request.mode === 'planning'
                                ? 'border-sky-500/20 bg-sky-500/10 text-sky-200'
                                : 'border-emerald-500/20 bg-emerald-500/10 text-emerald-200'
                                }`}>
                                {request.mode}
                            </span>
                            <span className="truncate flex-1" title={preview}>
                                {preview}
                            </span>
                            <div className="flex items-center gap-1 shrink-0">
                                <button
                                    type="button"
                                    onClick={() => onEditRequest(index)}
                                    className="p-0.5 rounded text-zinc-500 hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) transition-colors"
                                    title={t('chat.queue.editQueuedRequest')}
                                >
                                    <Pencil className="w-3 h-3" />
                                </button>
                                <button
                                    type="button"
                                    onClick={() => onDeleteRequest(index)}
                                    className="p-0.5 rounded text-zinc-500 hover:text-red-400 hover:bg-red-500/10 transition-colors"
                                    title={t('chat.queue.deleteQueuedRequest')}
                                >
                                    <Trash2 className="w-3 h-3" />
                                </button>
                            </div>
                        </div>
                    );
                })}
            </div>
        </div>
    );
};

export const QueuePanel = React.memo(QueuePanelComponent);
