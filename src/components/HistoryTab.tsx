import React, { useEffect } from 'react';
import { Clock, MessageSquare, Loader2 } from 'lucide-react';
import { useHistory } from '../hooks/useHistory';
import type { ConversationSummary } from '../types/history';

interface HistoryTabProps {
    projectId: string;
    onSelectConversation: (sessionId: string) => void;
}

export const HistoryTab: React.FC<HistoryTabProps> = ({ projectId, onSelectConversation }) => {
    const { conversations, loading, error, fetchConversations } = useHistory();

    const getConversationTitle = (conversation: ConversationSummary) => {
        const title = (conversation.title || '').trim();
        if (title.length > 0) {
            return title;
        }

        const preview = (conversation.preview || '').trim();
        if (preview.length > 0) {
            return preview.slice(0, 80);
        }

        return `Conversation ${formatTimestamp(conversation.last_active_at)}`;
    };

    useEffect(() => {
        if (projectId) {
            fetchConversations(projectId);
        }
    }, [projectId, fetchConversations]);

    const formatTimestamp = (timestamp: string) => {
        if (!timestamp) {
            return 'Unknown';
        }

        const date = new Date(timestamp);
        if (Number.isNaN(date.getTime())) {
            return 'Unknown';
        }

        const now = new Date();
        const diffMs = now.getTime() - date.getTime();
        if (diffMs < 0) {
            return date.toLocaleString();
        }

        const diffMins = Math.floor(diffMs / 60000);
        const diffHours = Math.floor(diffMs / 3600000);
        const diffDays = Math.floor(diffMs / 86400000);

        if (diffMins < 1) return 'Just now';
        if (diffMins < 60) return `${diffMins}m ago`;
        if (diffHours < 24) return `${diffHours}h ago`;
        if (diffDays === 1) return 'Yesterday';
        if (diffDays < 7) return `${diffDays}d ago`;
        return date.toLocaleDateString();
    };

    if (loading && conversations.length === 0) {
        return (
            <div className="flex flex-1 items-center justify-center bg-[var(--bg-app)] px-4">
                <div className="flex flex-col items-center gap-4 rounded-3xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]/80 px-8 py-8 text-[var(--fg-tertiary)] shadow-[0_24px_70px_rgba(0,0,0,0.26)]">
                    <div className="flex h-14 w-14 items-center justify-center rounded-3xl border border-[var(--border-subtle)] bg-[var(--bg-app)]">
                        <Loader2 className="h-7 w-7 animate-spin" />
                    </div>
                    <p className="text-xs font-semibold uppercase tracking-[0.18em]">Loading conversations...</p>
                </div>
            </div>
        );
    }

    if (error) {
        return (
            <div className="flex flex-1 items-center justify-center bg-[var(--bg-app)] px-4">
                <div className="flex flex-col items-center gap-3 rounded-3xl border border-red-500/20 bg-red-500/5 px-8 py-8 text-red-300 shadow-[0_24px_70px_rgba(0,0,0,0.26)]">
                    <p className="text-xs font-semibold uppercase tracking-[0.18em]">Failed to load conversations</p>
                    <p className="text-[10px] opacity-70">{error}</p>
                </div>
            </div>
        );
    }

    if (conversations.length === 0) {
        return (
            <div className="flex flex-1 items-center justify-center bg-[var(--bg-app)] px-4">
                <div className="select-none flex flex-col items-center gap-4 rounded-3xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]/80 px-8 py-8 text-[var(--fg-tertiary)] shadow-[0_24px_70px_rgba(0,0,0,0.26)]">
                    <div className="flex h-16 w-16 items-center justify-center rounded-3xl border border-[var(--border-subtle)] bg-[var(--bg-app)]">
                        <Clock className="h-8 w-8 opacity-50" />
                    </div>
                    <div className="text-center">
                        <h3 className="mb-2 text-sm font-semibold uppercase tracking-[0.18em] text-[var(--fg-secondary)]">
                            No Conversations Yet
                        </h3>
                        <p className="text-xs opacity-70">
                            Start a new conversation to see it here
                        </p>
                    </div>
                </div>
            </div>
        );
    }

    const now = new Date();
    const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const startOfYesterday = new Date(startOfToday.getTime() - 24 * 60 * 60 * 1000);
    const startOfWeek = new Date(startOfToday.getTime() - 7 * 24 * 60 * 60 * 1000);

    const grouped = conversations.reduce<Record<string, ConversationSummary[]>>((acc, conversation) => {
        const date = new Date(conversation.last_active_at);
        let bucket = 'Older';
        if (!Number.isNaN(date.getTime())) {
            if (date >= startOfToday) {
                bucket = 'Today';
            } else if (date >= startOfYesterday) {
                bucket = 'Yesterday';
            } else if (date >= startOfWeek) {
                bucket = 'Previous 7 days';
            }
        }

        if (!acc[bucket]) {
            acc[bucket] = [];
        }
        acc[bucket].push(conversation);
        return acc;
    }, {});

    const orderedBuckets = ['Today', 'Yesterday', 'Previous 7 days', 'Older'] as const;

    return (
        <div className="flex-1 overflow-y-auto bg-[var(--bg-app)]">
            <div className="mx-auto max-w-4xl px-4 py-4">
                <div className="mb-4 rounded-3xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]/70 px-5 py-4 shadow-[0_18px_50px_rgba(0,0,0,0.18)]">
                    <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-[var(--fg-tertiary)]">
                        Conversation History
                    </div>
                    <div className="mt-1 text-sm font-semibold text-[var(--fg-primary)]">
                        Resume previous sessions and revisit older agent runs.
                    </div>
                </div>
                <div className="space-y-5">
                    {orderedBuckets.map((bucket) => {
                        const items = grouped[bucket] || [];
                        if (items.length === 0) {
                            return null;
                        }

                        return (
                            <div key={bucket} className="space-y-2">
                                <div className="px-1 text-[10px] font-semibold uppercase tracking-[0.18em] text-[var(--fg-tertiary)]">
                                    {bucket}
                                </div>
                                {items.map((conversation) => (
                                    <button
                                        key={conversation.id}
                                        onClick={() => onSelectConversation(conversation.id)}
                                        className="group w-full rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]/80 px-4 py-3 text-left transition-colors hover:bg-[var(--bg-surface-hover)] shadow-[0_12px_30px_rgba(0,0,0,0.12)]"
                                    >
                                        <div className="flex items-center gap-3">
                                            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-app)]">
                                                <MessageSquare className="h-4 w-4 text-[var(--fg-tertiary)] group-hover:text-[var(--fg-secondary)]" />
                                            </div>
                                            <div className="flex-1 min-w-0">
                                                <h4 className="text-sm font-medium text-[var(--fg-primary)] truncate">
                                                    {getConversationTitle(conversation)}
                                                </h4>
                                                <div className="mt-1 text-[11px] text-[var(--fg-tertiary)] truncate">
                                                    {conversation.preview || 'No preview available'}
                                                </div>
                                            </div>
                                            <div className="shrink-0 text-right text-[10px] text-[var(--fg-tertiary)]">
                                                <div>{formatTimestamp(conversation.last_active_at)}</div>
                                                <div className="mt-1">{conversation.message_count} msgs</div>
                                            </div>
                                        </div>
                                    </button>
                                ))}
                            </div>
                        );
                    })}
                </div>
            </div>
        </div>
    );
};
