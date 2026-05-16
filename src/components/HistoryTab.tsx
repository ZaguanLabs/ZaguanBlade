import React, { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Clock, MessageSquare, Loader2 } from 'lucide-react';
import { useHistory } from '../hooks/useHistory';
import type { ConversationSummary } from '../types/history';
import { ScrollArea } from './ui/ScrollArea';

interface HistoryTabProps {
    projectId: string;
    onSelectConversation: (sessionId: string) => void;
}

export const HistoryTab: React.FC<HistoryTabProps> = ({ projectId, onSelectConversation }) => {
    const { t } = useTranslation();
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

        return t('historyTab.conversationFallback', {
            timestamp: formatTimestamp(conversation.last_active_at),
        });
    };

    useEffect(() => {
        if (projectId) {
            fetchConversations(projectId);
        }
    }, [projectId, fetchConversations]);

    const formatTimestamp = (timestamp: string) => {
        if (!timestamp) {
            return t('historyTab.unknown');
        }

        const date = new Date(timestamp);
        if (Number.isNaN(date.getTime())) {
            return t('historyTab.unknown');
        }

        const now = new Date();
        const diffMs = now.getTime() - date.getTime();
        if (diffMs < 0) {
            return date.toLocaleString();
        }

        const diffMins = Math.floor(diffMs / 60000);
        const diffHours = Math.floor(diffMs / 3600000);
        const diffDays = Math.floor(diffMs / 86400000);

        if (diffMins < 1) return t('historyTab.justNow');
        if (diffMins < 60) return t('historyTab.minutesAgo', { count: diffMins });
        if (diffHours < 24) return t('historyTab.hoursAgo', { count: diffHours });
        if (diffDays === 1) return t('historyTab.yesterday');
        if (diffDays < 7) return t('historyTab.daysAgo', { count: diffDays });
        return date.toLocaleDateString();
    };

    if (loading && conversations.length === 0) {
        return (
            <div className="flex flex-1 items-center justify-center bg-(--bg-app) px-4">
                <div className="flex flex-col items-center gap-4 rounded-[calc(var(--panel-radius)*1.25)] border border-(--border-subtle) bg-(--bg-surface)/80 px-8 py-8 text-(--fg-tertiary) shadow-(--shadow-xl)">
                    <div className="flex h-14 w-14 items-center justify-center rounded-[calc(var(--panel-radius)*1.1)] border border-(--border-subtle) bg-(--bg-app)">
                        <Loader2 className="h-7 w-7 animate-spin" />
                    </div>
                    <p className="text-xs font-semibold uppercase tracking-[0.18em]">{t('historyTab.loading')}</p>
                </div>
            </div>
        );
    }

    if (error) {
        return (
            <div className="flex flex-1 items-center justify-center bg-(--bg-app) px-4">
                <div className="flex flex-col items-center gap-3 rounded-[calc(var(--panel-radius)*1.25)] border border-(--state-danger)/20 bg-[color-mix(in_srgb,var(--state-danger)_5%,transparent)] px-8 py-8 text-(--state-danger) shadow-(--shadow-xl)">
                    <p className="text-xs font-semibold uppercase tracking-[0.18em]">{t('historyTab.loadFailed')}</p>
                    <p className="text-[10px] opacity-70">{error}</p>
                </div>
            </div>
        );
    }

    if (conversations.length === 0) {
        return (
            <div className="flex flex-1 items-center justify-center bg-(--bg-app) px-4">
                <div className="select-none flex flex-col items-center gap-4 rounded-[calc(var(--panel-radius)*1.25)] border border-(--border-subtle) bg-(--bg-surface)/80 px-8 py-8 text-(--fg-tertiary) shadow-(--shadow-xl)">
                    <div className="flex h-16 w-16 items-center justify-center rounded-[calc(var(--panel-radius)*1.1)] border border-(--border-subtle) bg-(--bg-app)">
                        <Clock className="h-8 w-8 opacity-50" />
                    </div>
                    <div className="text-center">
                        <h3 className="mb-2 text-sm font-semibold uppercase tracking-[0.18em] text-(--fg-secondary)">
                            {t('historyTab.emptyTitle')}
                        </h3>
                        <p className="text-xs opacity-70">
                            {t('historyTab.emptyDescription')}
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
        let bucket = t('historyTab.bucketOlder');
        if (!Number.isNaN(date.getTime())) {
            if (date >= startOfToday) {
                bucket = t('historyTab.bucketToday');
            } else if (date >= startOfYesterday) {
                bucket = t('historyTab.bucketYesterday');
            } else if (date >= startOfWeek) {
                bucket = t('historyTab.bucketPrevious7Days');
            }
        }

        if (!acc[bucket]) {
            acc[bucket] = [];
        }
        acc[bucket].push(conversation);
        return acc;
    }, {});

    const orderedBuckets = [
        t('historyTab.bucketToday'),
        t('historyTab.bucketYesterday'),
        t('historyTab.bucketPrevious7Days'),
        t('historyTab.bucketOlder'),
    ] as const;

    return (
        <ScrollArea className="flex-1 bg-(--bg-app)">
            <div className="mx-auto max-w-4xl px-4 py-4">
                <div className="mb-4 rounded-[calc(var(--panel-radius)*1.1)] border border-(--border-subtle) bg-(--bg-surface)/70 px-5 py-4 shadow-(--shadow-md)">
                    <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-(--fg-tertiary)">
                        {t('historyTab.title')}
                    </div>
                    <div className="mt-1 text-sm font-semibold text-(--fg-primary)">
                        {t('historyTab.subtitle')}
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
                                <div className="px-1 text-[10px] font-semibold uppercase tracking-[0.18em] text-(--fg-tertiary)">
                                    {bucket}
                                </div>
                                {items.map((conversation) => (
                                    <button
                                        key={conversation.id}
                                        onClick={() => onSelectConversation(conversation.id)}
                                        className="group w-full rounded-[calc(var(--panel-radius)*0.9)] border border-(--border-subtle) bg-(--bg-surface)/80 px-4 py-3 text-left transition-colors hover:bg-(--bg-surface-hover) shadow-(--shadow-sm)"
                                    >
                                        <div className="flex items-center gap-3">
                                            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-app)">
                                                <MessageSquare className="h-4 w-4 text-(--fg-tertiary) group-hover:text-(--fg-secondary)" />
                                            </div>
                                            <div className="flex-1 min-w-0">
                                                <h4 className="text-sm font-medium text-(--fg-primary) truncate">
                                                    {getConversationTitle(conversation)}
                                                </h4>
                                                <div className="mt-1 text-[11px] text-(--fg-tertiary) truncate">
                                                    {conversation.preview || t('historyTab.noPreview')}
                                                </div>
                                            </div>
                                            <div className="shrink-0 text-right text-[10px] text-(--fg-tertiary)">
                                                <div>{formatTimestamp(conversation.last_active_at)}</div>
                                                <div className="mt-1">{t('historyTab.messageCount', { count: conversation.message_count })}</div>
                                            </div>
                                        </div>
                                    </button>
                                ))}
                            </div>
                        );
                    })}
                </div>
            </div>
        </ScrollArea>
    );
};
