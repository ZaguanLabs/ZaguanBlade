import React from 'react';
import { Plus } from 'lucide-react';

interface ChatTabBarProps {
    activeTab: 'chat' | 'history';
    onTabChange: (tab: 'chat' | 'history') => void;
    onNewConversation: () => void;
}

export const ChatTabBar: React.FC<ChatTabBarProps> = ({ activeTab, onTabChange, onNewConversation }) => {
    return (
        <div className="h-9 border-b border-[var(--border-subtle)] flex items-center justify-between px-2 bg-[var(--bg-app)] select-none shrink-0">
            <div className="flex items-center gap-1">
                <button
                    onClick={() => onTabChange('chat')}
                    className={`
                        px-3 py-1 text-[11px] font-medium rounded transition-colors
                        ${activeTab === 'chat'
                            ? 'bg-[var(--bg-surface)] text-[var(--fg-primary)] border border-[var(--border-subtle)]'
                            : 'text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface)]/50'
                        }
                    `}
                >
                    Chat
                </button>
                <button
                    onClick={() => onTabChange('history')}
                    className={`
                        px-3 py-1 text-[11px] font-medium rounded transition-colors
                        ${activeTab === 'history'
                            ? 'bg-[var(--bg-surface)] text-[var(--fg-primary)] border border-[var(--border-subtle)]'
                            : 'text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-surface)]/50'
                        }
                    `}
                >
                    History
                </button>
            </div>
            <button
                onClick={onNewConversation}
                className="p-1 rounded hover:bg-[var(--bg-surface)] text-[var(--fg-secondary)] hover:text-[var(--fg-primary)] transition-colors"
                title="New Conversation"
            >
                <Plus className="w-4 h-4" />
            </button>
        </div>
    );
};
