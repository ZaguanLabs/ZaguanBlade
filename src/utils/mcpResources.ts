import type { ResourceAttachment } from '../types/chat';
import type { McpResourceRead } from '../types/integrations';

export const MAX_RESOURCE_ATTACHMENTS = 8;
/** Mirrors the backend bound on attached resource text per message. */
export const MAX_RESOURCE_TEXT_BYTES = 256 * 1024;

const encoder = new TextEncoder();
export const utf8Bytes = (text: string) => encoder.encode(text).length;

export interface ResourceSelection {
    attachment: ResourceAttachment | null;
    images: { dataUrl: string; name: string }[];
    unsupported: number;
}

/** Text parts become one attachment; images go through the image attachment
 * path; other binary content is counted so the UI can say it was left out. */
export function selectionFromRead(server: string, name: string, read: McpResourceRead): ResourceSelection {
    const texts = read.parts.filter(part => part.kind === 'text');
    const several = texts.length > 1;
    const text = texts.map(part => several ? `--- ${part.uri} ---\n${part.text}` : part.text).join('\n\n');
    const images = read.parts.flatMap(part => part.kind === 'image'
        ? [{ dataUrl: `data:${part.mime_type};base64,${part.data}`, name: part.uri.split('/').pop() || name }]
        : []);
    return {
        attachment: texts.length === 0 ? null : {
            id: crypto.randomUUID(),
            server,
            name,
            uri: read.uri,
            mime_type: texts[0].mime_type,
            text,
            bytes: utf8Bytes(text),
            truncated: read.truncated,
        },
        images,
        unsupported: read.parts.filter(part => part.kind === 'unsupported').length,
    };
}

export type AttachRefusal = 'count' | 'size' | 'duplicate';

export function attachRefusal(current: ResourceAttachment[], next: ResourceAttachment): AttachRefusal | null {
    if (current.some(item => item.server === next.server && item.uri === next.uri)) return 'duplicate';
    if (current.length >= MAX_RESOURCE_ATTACHMENTS) return 'count';
    const total = current.reduce((sum, item) => sum + item.bytes, 0);
    return total + next.bytes > MAX_RESOURCE_TEXT_BYTES ? 'size' : null;
}
