import React, { useEffect } from 'react';
import { Clock, MessageSquare, Loader2 } from 'lucide-react';
import { useHistory } from '../hooks/useHistory';
import type { ConversationSummary } from '../types/history';

interface HistoryTabProps {
    userId: string;
    projectId: string;
    onSelectConversation: (sessionId: string) => void;
}

export const HistoryTab: React.FC<HistoryTabProps> = ({ userId, projectId, onSelectConversation }) => {
    const { conversations, loading, error, fetchConversations } = useHistory();

    useEffect(() => {
        console.log('[HistoryTab] userId:', userId, 'projectId:', projectId);
        console.log('[HistoryTab] conversations.length:', conversations.length);
        
        if (userId && projectId) {
            console.log('[HistoryTab] Fetching conversations...');
            fetchConversations(userId, projectId);
        } else {
            console.warn('[HistoryTab] Missing userId or projectId, not fetching');
        }
    }, [userId, projectId, fetchConversations]);

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
            <div className="flex-1 flex items-center justify-center bg-[var(--bg-app)]">
                <div className="flex flex-col items-center gap-3 text-[var(--fg-tertiary)]">
                    <Loader2 className="w-8 h-8 animate-spin" />
                    <p className="text-xs">Loading conversations...</p>
                </div>
            </div>
        );
    }

    if (error) {
        return (
            <div className="flex-1 flex items-center justify-center bg-[var(--bg-app)]">
                <div className="flex flex-col items-center gap-3 text-red-400">
                    <p className="text-xs">Failed to load conversations</p>
                    <p className="text-[10px] opacity-70">{error}</p>
                </div>
            </div>
        );
    }

    if (conversations.length === 0) {
        return (
            <div className="flex-1 flex items-center justify-center bg-[var(--bg-app)]">
                <div className="flex flex-col items-center gap-4 text-[var(--fg-tertiary)] select-none">
                    <Clock className="w-12 h-12 opacity-20" />
                    <div className="text-center">
                        <h3 className="text-sm font-medium text-[var(--fg-secondary)] mb-1">
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
            <div className="max-w-4xl mx-auto py-3 px-3">
                <div className="space-y-4">
                    {orderedBuckets.map((bucket) => {
                        const items = grouped[bucket] || [];
                        if (items.length === 0) {
                            return null;
                        }

                        return (
                            <div key={bucket} className="space-y-1.5">
                                <div className="text-[11px] uppercase tracking-wide text-[var(--fg-tertiary)] px-1">
                                    {bucket}
                                </div>
                                {items.map((conversation) => (
                                    <button
                                        key={conversation.id}
                                        onClick={() => onSelectConversation(conversation.id)}
                                        className="w-full text-left px-3 py-2 rounded-md bg-[var(--bg-surface)] hover:bg-[var(--bg-surface-hover)] border border-[var(--border-subtle)] transition-colors group"
                                    >
                                        <div className="flex items-center gap-2.5">
                                            <div className="shrink-0">
                                                <MessageSquare className="w-3.5 h-3.5 text-[var(--fg-tertiary)] group-hover:text-[var(--fg-secondary)]" />
                                            </div>
                                            <div className="flex-1 min-w-0">
                                                <h4 className="text-sm font-medium text-[var(--fg-primary)] truncate">
                                                    {conversation.title}
                                                </h4>
                                            </div>
                                            <div className="flex items-center gap-2.5 text-[10px] text-[var(--fg-tertiary)] shrink-0">
                                                <span>{conversation.message_count} msgs</span>
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
