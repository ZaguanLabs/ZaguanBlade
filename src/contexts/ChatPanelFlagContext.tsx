import React, { createContext, useContext, useMemo, useState } from 'react';
import { useOptionalStartupBootstrap } from './StartupBootstrapContext';

interface ChatPanelFlagContextValue {
    chatPanelV3: boolean;
}

const ChatPanelFlagContext = createContext<ChatPanelFlagContextValue | null>(null);

function parseFlagValue(value: unknown): boolean | null {
    if (value === true || value === '1' || value === 'true') {
        return true;
    }
    if (value === false || value === '0' || value === 'false') {
        return false;
    }
    return null;
}

function readLocalOverride(): boolean | null {
    if (typeof window === 'undefined') {
        return null;
    }

    try {
        return parseFlagValue(window.localStorage.getItem('chat.panel.v3'));
    } catch {
        return null;
    }
}

function readRemoteFlag(remoteSettings: unknown): boolean | null {
    const value = (remoteSettings as { configuration?: Record<string, unknown>; feature_flags?: Record<string, unknown> } | null)?.configuration?.['chat.panel.v3']
        ?? (remoteSettings as { configuration?: Record<string, unknown>; feature_flags?: Record<string, unknown> } | null)?.feature_flags?.['chat.panel.v3'];
    return parseFlagValue(value);
}

export const ChatPanelFlagProvider: React.FC<React.PropsWithChildren> = ({ children }) => {
    const bootstrapContext = useOptionalStartupBootstrap();
    const bootstrap = bootstrapContext?.bootstrap ?? null;
    const [localOverride] = useState(readLocalOverride);
    const remoteFlag = readRemoteFlag(bootstrap?.remote_settings ?? null);
    const chatPanelV3 = localOverride ?? remoteFlag ?? true;

    const value = useMemo<ChatPanelFlagContextValue>(() => ({ chatPanelV3 }), [chatPanelV3]);

    return (
        <ChatPanelFlagContext.Provider value={value}>
            {children}
        </ChatPanelFlagContext.Provider>
    );
};

export function useChatPanelV3Flag(): boolean {
    return useContext(ChatPanelFlagContext)?.chatPanelV3 ?? false;
}
