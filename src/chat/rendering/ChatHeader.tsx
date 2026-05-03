import React from 'react';
import { Plus } from 'lucide-react';
import { ChatTabBar } from '../../components/ChatTabBar';

interface ChatHeaderProps {
    activeTab: 'chat' | 'history';
    onTabChange: (tab: 'chat' | 'history') => void;
    onNewConversation: () => void;
}

export const ChatHeader: React.FC<ChatHeaderProps> = ({ activeTab, onTabChange, onNewConversation }) => (
    <div className="shrink-0">
        <ChatTabBar
            activeTab={activeTab}
            onTabChange={onTabChange}
            onNewConversation={onNewConversation}
        />
        <button type="button" className="sr-only" onClick={onNewConversation}>
            <Plus className="h-3 w-3" />
        </button>
    </div>
);
