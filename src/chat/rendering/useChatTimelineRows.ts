import { useMemo, useRef } from 'react';
import type { ChatMessage as ChatMessageType, HookApprovalRequest } from '../../types/chat';
import type { StructuredAction } from '../../types/events';
import {
    computeStableChatTimelineRows,
    deriveChatActiveWorkState,
    deriveChatTimelineRowsFromProjection,
    type ChatActivity,
    type ChatActiveWorkState,
    type ChatProjection,
    type DerivedChatTimelineRow,
    type StableChatTimelineRowsState,
} from '../../utils/chatTimeline';
import { useChatProjection } from './useChatProjection';

interface UseChatTimelineRowsInput {
    messages: ChatMessageType[];
    activities?: ChatActivity[];
    loading: boolean;
    pendingActions: StructuredAction[] | null;
    pendingApprovalRequest?: HookApprovalRequest | null;
}

interface UseChatTimelineRowsResult {
    projection: ChatProjection;
    activeWorkState: ChatActiveWorkState;
    rows: DerivedChatTimelineRow[];
}

export function useChatTimelineRows(input: UseChatTimelineRowsInput): UseChatTimelineRowsResult {
    const {
        messages,
        activities = [],
        loading,
        pendingActions,
        pendingApprovalRequest,
    } = input;
    const stableRowsStateRef = useRef<StableChatTimelineRowsState>({ byKey: new Map(), rows: [] });
    const projection = useChatProjection(messages, activities);
    const activeWorkState = useMemo(() => deriveChatActiveWorkState(projection), [projection]);
    const rows = useMemo(() => {
        const rawRows = deriveChatTimelineRowsFromProjection(projection, loading, pendingActions, pendingApprovalRequest);
        const stableRowsState = computeStableChatTimelineRows(rawRows, stableRowsStateRef.current);
        stableRowsStateRef.current = stableRowsState;
        return stableRowsState.rows;
    }, [loading, pendingActions, pendingApprovalRequest, projection]);

    return useMemo(
        () => ({ projection, activeWorkState, rows }),
        [activeWorkState, projection, rows],
    );
}
