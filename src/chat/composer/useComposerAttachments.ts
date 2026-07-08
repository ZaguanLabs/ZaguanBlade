import { useCallback, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { readFile } from '@tauri-apps/plugin-fs';
import i18n from '../../i18n';
import type { ImageAttachment } from '../../types/chat';
import {
    createThumbnailDataUrl,
    extractBase64FromDataUrl,
    extractMimeTypeFromDataUrl,
    fileToDataUrl,
    getBase64ByteLength,
    validateImageByteLength,
    validateImageMimeType,
} from '../../utils/imageUtils';

const MIME_BY_EXT: Record<string, string> = {
    png: 'image/png',
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    webp: 'image/webp',
    gif: 'image/gif',
};

function isImageLike(file: File): boolean {
    return file.type.startsWith('image/')
        || /\.(png|jpe?g|webp|gif)$/i.test(file.name)
        || file.type === '';
}

async function attachmentFromDataUrl(dataUrl: string, name: string): Promise<ImageAttachment> {
    const data = extractBase64FromDataUrl(dataUrl);
    const mimeType = extractMimeTypeFromDataUrl(dataUrl) || 'image/png';
    const mimeError = validateImageMimeType(mimeType);
    if (mimeError) {
        throw new Error(mimeError);
    }
    const size = getBase64ByteLength(data);
    const sizeError = validateImageByteLength(size);
    if (sizeError) {
        throw new Error(sizeError);
    }
    return {
        id: crypto.randomUUID(),
        dataUrl,
        data,
        mime_type: mimeType,
        thumbnailUrl: await createThumbnailDataUrl(dataUrl, 64, 64),
        name,
        size,
    };
}

export function useComposerAttachments(options: {
    disabledImages: boolean;
    disabledReason?: string;
}) {
    const [attachments, setAttachments] = useState<ImageAttachment[]>([]);
    const [attachmentError, setAttachmentError] = useState<string | null>(null);

    const appendFiles = useCallback(async (files: File[]) => {
        if (files.length === 0) {
            return;
        }
        if (options.disabledImages) {
            setAttachmentError(options.disabledReason || i18n.t('chat.attachments.imagesNotAvailable', { defaultValue: 'Images are not available for this model.' }));
            return;
        }

        const next: ImageAttachment[] = [];
        for (const file of files.filter(isImageLike)) {
            try {
                next.push(await attachmentFromDataUrl(await fileToDataUrl(file), file.name || 'image.png'));
            } catch (error) {
                setAttachmentError(error instanceof Error ? error.message : String(error));
            }
        }

        if (next.length > 0) {
            setAttachments((current) => [...current, ...next]);
            setAttachmentError(null);
        }
    }, [options.disabledImages, options.disabledReason]);

    const appendCapture = useCallback(async (result: { data: string; mime_type: string }, name: string) => {
        if (options.disabledImages) {
            setAttachmentError(options.disabledReason || i18n.t('chat.attachments.imagesNotAvailable', { defaultValue: 'Images are not available for this model.' }));
            return false;
        }
        try {
            const dataUrl = `data:${result.mime_type};base64,${result.data}`;
            const attachment = await attachmentFromDataUrl(dataUrl, name);
            setAttachments((current) => [...current, attachment]);
            setAttachmentError(null);
            return true;
        } catch (error) {
            setAttachmentError(error instanceof Error ? error.message : String(error));
            return false;
        }
    }, [options.disabledImages, options.disabledReason]);

    const appendDataUrl = useCallback(async (dataUrl: string, name: string) => {
        if (options.disabledImages) {
            setAttachmentError(options.disabledReason || i18n.t('chat.attachments.imagesNotAvailable', { defaultValue: 'Images are not available for this model.' }));
            return false;
        }
        try {
            const attachment = await attachmentFromDataUrl(dataUrl, name);
            setAttachments((current) => [...current, attachment]);
            setAttachmentError(null);
            return true;
        } catch (error) {
            setAttachmentError(error instanceof Error ? error.message : String(error));
            return false;
        }
    }, [options.disabledImages, options.disabledReason]);

    const uploadImages = useCallback(async () => {
        if (options.disabledImages) {
            setAttachmentError(options.disabledReason || i18n.t('chat.attachments.imagesNotAvailable', { defaultValue: 'Images are not available for this model.' }));
            return;
        }

        const selected = await open({
            multiple: true,
            filters: [{ name: i18n.t('chat.attachments.imagesFilter', { defaultValue: 'Images' }), extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'] }],
        });
        if (!selected) {
            return;
        }

        const paths = Array.isArray(selected) ? selected : [selected];
        const next: ImageAttachment[] = [];
        for (const path of paths) {
            try {
                const bytes = await readFile(path);
                const ext = path.split('.').pop()?.toLowerCase() || 'png';
                const mimeType = MIME_BY_EXT[ext] || 'image/png';
                const binary = Array.from(bytes, (byte) => String.fromCharCode(byte)).join('');
                next.push(await attachmentFromDataUrl(`data:${mimeType};base64,${btoa(binary)}`, path.split(/[/\\]/).pop() || 'image.png'));
            } catch (error) {
                setAttachmentError(error instanceof Error ? error.message : String(error));
            }
        }
        if (next.length > 0) {
            setAttachments((current) => [...current, ...next]);
            setAttachmentError(null);
        }
    }, [options.disabledImages, options.disabledReason]);

    const removeAttachment = useCallback((id: string) => {
        setAttachments((current) => current.filter((attachment) => attachment.id !== id));
    }, []);

    const clearAttachments = useCallback(() => {
        setAttachments([]);
        setAttachmentError(null);
    }, []);

    return {
        attachments,
        attachmentError,
        appendFiles,
        appendCapture,
        appendDataUrl,
        uploadImages,
        removeAttachment,
        clearAttachments,
        setAttachmentError,
    };
}
