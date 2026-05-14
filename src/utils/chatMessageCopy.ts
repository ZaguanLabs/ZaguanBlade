import type { ChatMessage } from '../types/chat';

export function getCopyableMessageContent(message: Pick<ChatMessage, 'role' | 'content'>) {
    const content = message.content ?? '';
    const roleHeaderPattern = new RegExp(`^(?:\\*\\*)?${message.role}(?::)?(?:\\*\\*)?\\r?\\n\\r?\\n`, 'i');
    return content.replace(roleHeaderPattern, '');
}
