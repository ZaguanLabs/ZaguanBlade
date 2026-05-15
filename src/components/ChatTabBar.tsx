import React from 'react';
import { useTranslation } from 'react-i18next';
import { Plus } from 'lucide-react';

interface ChatTabBarProps {
    activeTab: 'chat' | 'history';
    onTabChange: (tab: 'chat' | 'history') => void;
    onNewConversation: () => void;
}

export const ChatTabBar: React.FC<ChatTabBarProps> = ({ activeTab, onTabChange, onNewConversation }) => {
    const { t } = useTranslation();
    return (
        <div className="flex h-12 shrink-0 items-center justify-between border-b border-(--border-subtle) bg-(--bg-panel) px-3 select-none">
            <div className="flex items-center gap-1 rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface)/55 p-1 shadow-(--shadow-sm)">
                <button
                    onClick={() => onTabChange('chat')}
                    className={`
                        rounded-[calc(var(--panel-radius)*0.55)] px-3 py-1.5 text-[11px] font-semibold transition-colors
                        ${activeTab === 'chat'
                            ? 'border border-(--border-subtle) bg-(--bg-app) text-(--fg-primary) shadow-(--shadow-sm)'
                            : 'text-(--fg-secondary) hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)'
                        }
                    `}
                >
                    {t('chat.tabs.chat')}
                </button>
                <button
                    onClick={() => onTabChange('history')}
                    className={`
                        rounded-[calc(var(--panel-radius)*0.55)] px-3 py-1.5 text-[11px] font-semibold transition-colors
                        ${activeTab === 'history'
                            ? 'border border-(--border-subtle) bg-(--bg-app) text-(--fg-primary) shadow-(--shadow-sm)'
                            : 'text-(--fg-secondary) hover:bg-(--bg-surface-hover) hover:text-(--fg-primary)'
                        }
                    `}
                >
                    {t('chat.tabs.history')}
                </button>
            </div>
            <button
                onClick={onNewConversation}
                className="inline-flex h-9 w-9 items-center justify-center rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface)/70 text-(--fg-secondary) transition-colors hover:border-[color-mix(in_srgb,var(--accent-ai)_32%,transparent)] hover:text-(--fg-primary)"
                title={t('chat.newConversation')}
            >
                <Plus className="w-4 h-4" />
            </button>
        </div>
    );
};
