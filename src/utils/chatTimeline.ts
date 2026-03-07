import type { ChatMessage as ChatMessageType } from '../types/chat';
import type { StructuredAction } from '../types/events';

export interface DerivedChatMessageRow {
    kind: 'message';
    key: string;
    message: ChatMessageType;
    isContinued: boolean;
    isActive: boolean;
    pendingActions?: StructuredAction[];
}

export type DerivedChatRow = DerivedChatMessageRow;

export function deriveChatRows(
    messages: ChatMessageType[],
    loading: boolean,
    pendingActions: StructuredAction[] | null,
): DerivedChatRow[] {
    return messages.map((message, index) => {
        const isLast = index === messages.length - 1;
        const isAssistant = message.role === 'Assistant';
        const prevMessage = index > 0 ? messages[index - 1] : null;
        const isContinued = isAssistant && prevMessage?.role === 'Assistant';
        const showPendingActions = isLast && isAssistant && !!pendingActions && pendingActions.length > 0;

        return {
            kind: 'message',
            key: message.id || `${message.role}-${index}`,
            message,
            isContinued,
            isActive: isLast && loading,
            ...(showPendingActions ? { pendingActions } : {}),
        };
    });
}
