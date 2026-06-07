import React, { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus } from 'lucide-react';

interface ChatTabBarProps {
    activeTab: 'chat' | 'history';
    onTabChange: (tab: 'chat' | 'history') => void;
    onNewConversation: () => void;
}

export const ChatTabBar: React.FC<ChatTabBarProps> = ({ activeTab, onTabChange, onNewConversation }) => {
    const { t } = useTranslation();
    const chatTabRef = useRef<HTMLButtonElement>(null);
    const historyTabRef = useRef<HTMLButtonElement>(null);
    const focusActiveTabAfterKeyboardRef = useRef(false);

    useEffect(() => {
        if (!focusActiveTabAfterKeyboardRef.current) {
            return;
        }

        const nextTab = activeTab === 'chat' ? chatTabRef.current : historyTabRef.current;
        nextTab?.focus();
        focusActiveTabAfterKeyboardRef.current = false;
    }, [activeTab]);

    const selectTabFromKeyboard = (tab: 'chat' | 'history') => {
        focusActiveTabAfterKeyboardRef.current = true;
        onTabChange(tab);
    };

    const handleTabKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
        if (event.key === 'ArrowLeft' || event.key === 'ArrowUp' || event.key === 'Home') {
            event.preventDefault();
            selectTabFromKeyboard('chat');
            return;
        }
        if (event.key === 'ArrowRight' || event.key === 'ArrowDown' || event.key === 'End') {
            event.preventDefault();
            selectTabFromKeyboard('history');
            return;
        }
    };

    return (
        <div className="flex h-12 shrink-0 items-center justify-between border-b border-(--border-subtle) bg-(--bg-panel) px-3 select-none">
            <div
                role="tablist"
                aria-label={t('chat.tabs.chat')}
                className="flex items-center gap-1 rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface)/55 p-1 shadow-(--shadow-sm)"
            >
                <button
                    ref={chatTabRef}
                    type="button"
                    role="tab"
                    aria-selected={activeTab === 'chat'}
                    tabIndex={activeTab === 'chat' ? 0 : -1}
                    onClick={() => onTabChange('chat')}
                    onKeyDown={handleTabKeyDown}
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
                    ref={historyTabRef}
                    type="button"
                    role="tab"
                    aria-selected={activeTab === 'history'}
                    tabIndex={activeTab === 'history' ? 0 : -1}
                    onClick={() => onTabChange('history')}
                    onKeyDown={handleTabKeyDown}
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
                type="button"
                onClick={onNewConversation}
                aria-label={t('chat.newConversation')}
                className="inline-flex h-9 w-9 items-center justify-center rounded-[calc(var(--panel-radius)*0.75)] border border-(--border-subtle) bg-(--bg-surface)/70 text-(--fg-secondary) transition-colors hover:border-[color-mix(in_srgb,var(--accent-ai)_32%,transparent)] hover:text-(--fg-primary)"
                title={t('chat.newConversation')}
            >
                <Plus className="w-4 h-4" aria-hidden="true" />
            </button>
        </div>
    );
};
