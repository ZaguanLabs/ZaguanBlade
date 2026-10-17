import type { ResourceAttachment } from '../types/chat';
import type { McpPromptRender } from '../types/integrations';
import { attachRefusal, utf8Bytes, type AttachRefusal } from './mcpResources';

export interface PromptInsertion {
    text: string;
    resources: ResourceAttachment[];
    images: { dataUrl: string; name: string }[];
    links: number;
    unsupported: number;
}

/** Text goes into the composer for the user to edit and send. Blade never
 * injects assistant turns it did not generate, so when a prompt mixes roles
 * each block is labelled with its role instead. Embedded resources become
 * resource attachments (fenced as external data); links are not fetched. */
export function insertionFromPrompt(
    server: string,
    render: McpPromptRender,
    labels: { user: string; assistant: string },
): PromptInsertion {
    const mixed = render.messages.some(message => message.role === 'assistant');
    const blocks: string[] = [];
    const insertion: PromptInsertion = { text: '', resources: [], images: [], links: 0, unsupported: 0 };
    for (const { role, part } of render.messages) {
        switch (part.kind) {
            case 'text':
                if (part.text.trim()) blocks.push(mixed ? `${labels[role]}:\n${part.text}` : part.text);
                break;
            case 'resource':
                insertion.resources.push({
                    id: crypto.randomUUID(),
                    server,
                    name: part.uri.split('/').filter(Boolean).pop() || part.uri,
                    uri: part.uri,
                    mime_type: part.mime_type,
                    text: part.text,
                    bytes: utf8Bytes(part.text),
                    truncated: render.truncated,
                });
                break;
            case 'image':
                insertion.images.push({ dataUrl: `data:${part.mime_type};base64,${part.data}`, name: `${render.name}-${insertion.images.length + 1}` });
                break;
            case 'link':
                insertion.links += 1;
                break;
            default:
                insertion.unsupported += 1;
        }
    }
    insertion.text = blocks.join('\n\n');
    return insertion;
}

/** All-or-nothing: the first resource that would be refused stops the insertion. */
export function insertionRefusal(current: ResourceAttachment[], insertion: PromptInsertion): AttachRefusal | null {
    let accepted = current;
    for (const resource of insertion.resources) {
        const refusal = attachRefusal(accepted, resource);
        if (refusal) return refusal;
        accepted = [...accepted, resource];
    }
    return null;
}

export const isEmptyInsertion = (insertion: PromptInsertion) =>
    !insertion.text && insertion.resources.length === 0 && insertion.images.length === 0;
