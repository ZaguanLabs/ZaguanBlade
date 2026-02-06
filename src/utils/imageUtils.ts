export const fileToDataUrl = (file: File): Promise<string> => new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(new Error('Failed to read image file'));
    reader.readAsDataURL(file);
});

export const MAX_IMAGE_BYTES = 10 * 1024 * 1024;
export const SUPPORTED_IMAGE_MIME_TYPES = new Set([
    'image/png',
    'image/jpeg',
    'image/webp',
    'image/gif'
]);

export const validateImageByteLength = (byteLength: number): string | null => {
    if (byteLength <= 0) {
        return 'Image is empty.';
    }
    if (byteLength > MAX_IMAGE_BYTES) {
        const sizeMb = (byteLength / (1024 * 1024)).toFixed(1);
        const limitMb = (MAX_IMAGE_BYTES / (1024 * 1024)).toFixed(0);
        return `Image too large (${sizeMb} MB). Max ${limitMb} MB. Try a smaller image.`;
    }
    return null;
};

export const validateImageSize = (file: File): string | null => {
    return validateImageByteLength(file.size);
};

export const validateImageMimeType = (mimeType?: string | null): string | null => {
    if (!mimeType) {
        return 'Missing image MIME type.';
    }
    if (!SUPPORTED_IMAGE_MIME_TYPES.has(mimeType)) {
        return `Unsupported image type (${mimeType}).`;
    }
    return null;
};

export const createThumbnailDataUrl = (
    dataUrl: string,
    maxWidth: number,
    maxHeight: number
): Promise<string> => new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
        let { width, height } = img;
        if (width > maxWidth || height > maxHeight) {
            const ratio = Math.min(maxWidth / width, maxHeight / height);
            width = Math.max(1, Math.round(width * ratio));
            height = Math.max(1, Math.round(height * ratio));
        }
        const canvas = document.createElement('canvas');
        canvas.width = width;
        canvas.height = height;
        const ctx = canvas.getContext('2d');
        if (!ctx) {
            resolve(dataUrl);
            return;
        }
        ctx.drawImage(img, 0, 0, width, height);
        resolve(canvas.toDataURL('image/jpeg', 0.82));
    };
    img.src = dataUrl;
});

export const extractBase64FromDataUrl = (dataUrl: string): string => {
    const commaIndex = dataUrl.indexOf(',');
    return commaIndex >= 0 ? dataUrl.slice(commaIndex + 1) : dataUrl;
};

export const extractMimeTypeFromDataUrl = (dataUrl: string): string | null => {
    const match = /^data:(.*?);base64,/.exec(dataUrl);
    return match?.[1] ?? null;
};

export const getBase64ByteLength = (base64: string): number => {
    if (!base64) return 0;
    const normalized = base64.replace(/=+$/, '');
    return Math.floor((normalized.length * 3) / 4);
};
