import React from 'react';
import { X } from 'lucide-react';
import type { ImageAttachment } from '../types/chat';

interface ImageAttachmentBarProps {
    attachments: ImageAttachment[];
    onRemove: (id: string) => void;
}

export const ImageAttachmentBar: React.FC<ImageAttachmentBarProps> = ({ attachments, onRemove }) => {
    if (attachments.length === 0) return null;

    return (
        <div className="px-2 py-2 border-b border-[var(--border-subtle)]/60 bg-[var(--bg-app)]">
            <div className="flex items-center gap-2 overflow-x-auto scrollbar-thin scrollbar-thumb-zinc-800 scrollbar-track-transparent">
                {attachments.map((attachment) => (
                    <div
                        key={attachment.id}
                        className="relative group w-12 h-12 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] overflow-hidden shrink-0"
                    >
                        <img
                            src={attachment.thumbnailUrl}
                            alt={attachment.name || 'Attachment'}
                            className="w-full h-full object-cover"
                        />
                        <button
                            type="button"
                            onClick={() => onRemove(attachment.id)}
                            className="absolute top-0.5 right-0.5 p-0.5 rounded-full bg-black/60 text-white opacity-0 group-hover:opacity-100 transition-opacity"
                            aria-label="Remove image"
                        >
                            <X className="w-3 h-3" />
                        </button>
                    </div>
                ))}
            </div>
        </div>
    );
};
