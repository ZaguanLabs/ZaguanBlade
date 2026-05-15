import React from 'react';
import { ImageAttachmentBar } from '../../components/ImageAttachmentBar';
import type { ImageAttachment } from '../../types/chat';

export const AttachmentControls: React.FC<{
    attachments: ImageAttachment[];
    error: string | null;
    onRemove: (id: string) => void;
}> = ({ attachments, error, onRemove }) => (
    <>
        <ImageAttachmentBar attachments={attachments} onRemove={onRemove} />
        {error && <div className="px-2 pb-1 pt-1 text-[10px] text-(--state-danger)">{error}</div>}
    </>
);
