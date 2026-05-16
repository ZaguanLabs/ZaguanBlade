import React from 'react';
import { useTranslation } from 'react-i18next';
import { ListOrdered, Pencil, Trash2 } from 'lucide-react';
import type { QueuedRequest } from '../types/chat';
import { ScrollArea } from './ui/ScrollArea';
import { IconButton } from './ui/IconButton';
import { Surface } from './ui/Surface';

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
                <div className="flex h-7 w-7 items-center justify-center rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-app)/40">
                    <ListOrdered className="h-3.5 w-3.5 text-(--fg-tertiary)" />
                </div>
                <span className="font-semibold uppercase tracking-[0.16em]">{t('chat.queue.title', { count: requests.length })}</span>
            </div>
            <ScrollArea className="max-h-[200px] space-y-1 overflow-auto px-3 pb-3">
                {requests.map((request, index) => {
                    const preview = request.text.trim() || t('chat.queue.imageOnlyRequest');
                    return (
                        <Surface
                            key={`queued-${index}`}
                            variant="row"
                            className="flex items-center gap-2 px-2.5 py-2 text-[11px] text-(--fg-tertiary)"
                        >
                            <span className="font-mono text-[10px] text-(--fg-tertiary) w-4 text-right shrink-0">
                                {index + 1}.
                            </span>
                            <span className={`inline-flex shrink-0 items-center rounded-md border px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.12em] ${request.mode === 'planning'
                                ? 'border-(--accent-planning)/20 bg-[color-mix(in_srgb,var(--accent-planning)_10%,transparent)] text-(--accent-planning)'
                                : 'border-(--accent-mention)/20 bg-[color-mix(in_srgb,var(--accent-mention)_10%,transparent)] text-(--accent-mention)'
                                }`}>
                                {request.mode}
                            </span>
                            <span className="truncate flex-1" title={preview}>
                                {preview}
                            </span>
                            <div className="flex items-center gap-1 shrink-0">
                                <IconButton
                                    size="xs"
                                    tone="muted"
                                    onClick={() => onEditRequest(index)}
                                    title={t('chat.queue.editQueuedRequest')}
                                >
                                    <Pencil className="w-3 h-3" />
                                </IconButton>
                                <IconButton
                                    size="xs"
                                    tone="danger"
                                    onClick={() => onDeleteRequest(index)}
                                    title={t('chat.queue.deleteQueuedRequest')}
                                >
                                    <Trash2 className="w-3 h-3" />
                                </IconButton>
                            </div>
                        </Surface>
                    );
                })}
            </ScrollArea>
        </div>
    );
};

export const QueuePanel = React.memo(QueuePanelComponent);
