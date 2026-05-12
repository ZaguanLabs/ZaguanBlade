import { useMemo, useRef } from 'react';
import type { ChatMessage as ChatMessageType } from '../../types/chat';
import { deriveChatProjection, stabilizeChatProjection, type ChatActivity, type ChatProjection } from '../../utils/chatTimeline';

export function useChatProjection(
    messages: ChatMessageType[],
    activities: ChatActivity[] = [],
): ChatProjection {
    const previousProjectionRef = useRef<ChatProjection | null>(null);
    return useMemo(
        () => {
            const projection = stabilizeChatProjection(
                deriveChatProjection(messages, activities),
                previousProjectionRef.current,
            );
            previousProjectionRef.current = projection;
            return projection;
        },
        [activities, messages],
    );
}
