import React from 'react';
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
    </div>
);
