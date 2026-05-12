import { useMemo } from 'react';
import type { ChatMessage as ChatMessageType } from '../../types/chat';
import { deriveChatProjection, type ChatActivity, type ChatProjection } from '../../utils/chatTimeline';

export function useChatProjection(
    messages: ChatMessageType[],
    activities: ChatActivity[] = [],
): ChatProjection {
    return useMemo(
        () => deriveChatProjection(messages, activities),
        [activities, messages],
    );
}
