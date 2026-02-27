import React from 'react';
import { ListOrdered, Pencil, Trash2 } from 'lucide-react';
import type { QueuedRequest } from '../types/chat';

interface QueuePanelProps {
    requests: QueuedRequest[];
    onEditRequest: (index: number) => void;
    onDeleteRequest: (index: number) => void;
}

const QueuePanelComponent: React.FC<QueuePanelProps> = ({ requests, onEditRequest, onDeleteRequest }) => {
    if (requests.length === 0) return null;

    return (
        <div className="border-t border-(--border-subtle) bg-(--bg-surface)/70 backdrop-blur-sm">
            <div className="flex items-center gap-2 px-3 py-1.5 text-[11px] text-(--fg-secondary)">
                <ListOrdered className="w-3 h-3 text-zinc-400" />
                <span className="font-medium">Queued requests ({requests.length})</span>
            </div>
            <div className="px-3 pb-2 space-y-1 max-h-[170px] overflow-auto">
                {requests.map((request, index) => {
                    const preview = request.text.trim() || '(image-only request)';
                    return (
                        <div
                            key={`queued-${index}`}
                            className="flex items-center gap-2 text-[11px] text-zinc-400"
                        >
                            <span className="font-mono text-[10px] text-zinc-600 w-4 text-right shrink-0">
                                {index + 1}.
                            </span>
                            <span className="truncate flex-1" title={preview}>
                                {preview}
                            </span>
                            <div className="flex items-center gap-1 shrink-0">
                                <button
                                    type="button"
                                    onClick={() => onEditRequest(index)}
                                    className="p-0.5 rounded text-zinc-500 hover:text-(--fg-primary) hover:bg-(--bg-surface-hover) transition-colors"
                                    title="Edit queued request"
                                >
                                    <Pencil className="w-3 h-3" />
                                </button>
                                <button
                                    type="button"
                                    onClick={() => onDeleteRequest(index)}
                                    className="p-0.5 rounded text-zinc-500 hover:text-red-400 hover:bg-red-500/10 transition-colors"
                                    title="Delete queued request"
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
