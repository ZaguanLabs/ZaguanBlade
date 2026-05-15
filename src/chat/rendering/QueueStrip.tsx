import React from 'react';
import type { QueuedRequest } from '../../types/chat';
import { StatusStripFrame } from './StatusStripFrame';

interface QueueStripProps {
    requests: QueuedRequest[];
    onEditRequest?: (index: number) => void;
    onDeleteRequest?: (index: number) => void;
}

export const QueueStrip: React.FC<QueueStripProps> = ({ requests, onEditRequest, onDeleteRequest }) => {
    if (requests.length === 0) {
        return null;
    }

    return (
        <StatusStripFrame label="Queue" count={requests.length}>
            {requests.map((request, index) => (
                <div key={`${request.text}:${index}`} className="flex items-center gap-2 text-[11px] text-(--fg-secondary)">
                    <span className="min-w-0 flex-1 truncate">{request.text || `${request.attachments?.length ?? 0} attachment(s)`}</span>
                    <button type="button" onClick={() => onEditRequest?.(index)} className="text-(--fg-tertiary) hover:text-(--fg-primary)">Edit</button>
                    <button type="button" onClick={() => onDeleteRequest?.(index)} className="text-(--state-danger) hover:text-(--fg-primary)">Delete</button>
                </div>
            ))}
        </StatusStripFrame>
    );
};
