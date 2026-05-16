import React from 'react';
import { useTranslation } from 'react-i18next';
import { X } from 'lucide-react';
import type { ImageAttachment } from '../types/chat';

interface ImageAttachmentBarProps {
    attachments: ImageAttachment[];
    onRemove: (id: string) => void;
}

export const ImageAttachmentBar: React.FC<ImageAttachmentBarProps> = ({ attachments, onRemove }) => {
    const { t } = useTranslation();
    if (attachments.length === 0) return null;

    return (
        <div className="border-b border-(--border-subtle)/60 bg-(--bg-app)/60 px-3 py-3">
            <div className="mb-2 text-[10px] font-semibold uppercase tracking-[0.18em] text-(--fg-tertiary)">
                {t('chat.attachments.title')}
            </div>
            <div className="flex items-center gap-2 overflow-x-auto scrollbar-thin scrollbar-thumb-(--bg-surface-hover) scrollbar-track-transparent">
                {attachments.map((attachment) => (
                    <div
                        key={attachment.id}
                        className="group relative h-14 w-14 shrink-0 overflow-hidden rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface) shadow-(--shadow-sm)"
                    >
                        <img
                            src={attachment.thumbnailUrl}
                            alt={attachment.name || t('chat.attachments.attachmentAlt')}
                            className="w-full h-full object-cover"
                        />
                        <button
                            type="button"
                            onClick={() => onRemove(attachment.id)}
                            className="absolute right-1 top-1 rounded-full bg-(--bg-app)/75 p-1 text-(--fg-bright) opacity-0 transition-opacity group-hover:opacity-100"
                            aria-label={t('chat.attachments.removeImage')}
                        >
                            <X className="w-3 h-3" />
                        </button>
                    </div>
                ))}
            </div>
        </div>
    );
};
