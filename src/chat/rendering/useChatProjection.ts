import { useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import type { ChatMessage as ChatMessageType } from '../../types/chat';
import { deriveChatProjection, stabilizeChatProjection, type ChatActivity, type ChatProjection } from '../../utils/chatTimeline';

export function useChatProjection(
    messages: ChatMessageType[],
    activities: ChatActivity[] = [],
): ChatProjection {
    const { i18n } = useTranslation();
    const previousProjectionRef = useRef<ChatProjection | null>(null);
    const messageWorkEntryCacheRef = useRef({
        language: i18n.resolvedLanguage,
        entries: new WeakMap<ChatMessageType, ChatProjection['workEntries']>(),
    });
    return useMemo(
        () => {
            if (messageWorkEntryCacheRef.current.language !== i18n.resolvedLanguage) {
                messageWorkEntryCacheRef.current = {
                    language: i18n.resolvedLanguage,
                    entries: new WeakMap(),
                };
            }
            const projection = stabilizeChatProjection(
                deriveChatProjection(messages, activities, messageWorkEntryCacheRef.current.entries),
                previousProjectionRef.current,
            );
            previousProjectionRef.current = projection;
            return projection;
        },
        [activities, i18n.resolvedLanguage, messages],
    );
}
