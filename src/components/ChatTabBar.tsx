import React from 'react';
import { Plus } from 'lucide-react';

interface ChatTabBarProps {
    activeTab: 'chat' | 'history';
    onTabChange: (tab: 'chat' | 'history') => void;
    onNewConversation: () => void;
}

export const ChatTabBar: React.FC<ChatTabBarProps> = ({ activeTab, onTabChange, onNewConversation }) => {
    return (
        <div className="flex h-12 shrink-0 items-center justify-between border-b border-[var(--border-subtle)] bg-[var(--bg-app)]/90 px-3 select-none backdrop-blur-md">
            <div className="flex items-center gap-2 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]/60 p-1">
                <button
                    onClick={() => onTabChange('chat')}
                    className={`
                        rounded-xl px-3 py-1.5 text-[11px] font-semibold transition-colors
                        ${activeTab === 'chat'
                            ? 'border border-[var(--border-subtle)] bg-[var(--bg-app)] text-[var(--fg-primary)] shadow-[0_8px_20px_rgba(0,0,0,0.18)]'
                            : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface)]/70 hover:text-[var(--fg-primary)]'
                        }
                    `}
                >
                    Chat
                </button>
                <button
                    onClick={() => onTabChange('history')}
                    className={`
                        rounded-xl px-3 py-1.5 text-[11px] font-semibold transition-colors
                        ${activeTab === 'history'
                            ? 'border border-[var(--border-subtle)] bg-[var(--bg-app)] text-[var(--fg-primary)] shadow-[0_8px_20px_rgba(0,0,0,0.18)]'
                            : 'text-[var(--fg-secondary)] hover:bg-[var(--bg-surface)]/70 hover:text-[var(--fg-primary)]'
                        }
                    `}
                >
                    History
                </button>
            </div>
            <button
                onClick={onNewConversation}
                className="inline-flex h-9 w-9 items-center justify-center rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] text-[var(--fg-secondary)] transition-colors hover:border-[var(--accent-primary)]/30 hover:text-[var(--fg-primary)]"
                title="New Conversation"
            >
                <Plus className="w-4 h-4" />
            </button>
        </div>
    );
};
