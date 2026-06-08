import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { useOptionalStartupBootstrap } from './StartupBootstrapContext';
import type { RemoteAiConfig } from '../types/settings';

export const MIN_CONTENT_FONT_SIZE = 10;
export const MAX_CONTENT_FONT_SIZE = 22;
export const DEFAULT_EDITOR_FONT_SIZE = 14;
export const DEFAULT_CHAT_FONT_SIZE = 13;

type FontSizeTarget = 'editor' | 'chat';

interface DisplaySettingsContextValue {
    editorFontSize: number;
    chatFontSize: number;
    setEditorFontSize: (fontSize: number) => void;
    setChatFontSize: (fontSize: number) => void;
    adjustFontSize: (target: FontSizeTarget, delta: number) => void;
}

const DisplaySettingsContext = createContext<DisplaySettingsContextValue | null>(null);

function clampFontSize(value: number, fallback: number) {
    if (!Number.isFinite(value) || value <= 0) {
        return fallback;
    }
    return Math.max(MIN_CONTENT_FONT_SIZE, Math.min(MAX_CONTENT_FONT_SIZE, Math.round(value)));
}

function normalizeRemoteFontSettings(settings: RemoteAiConfig) {
    return {
        editorFontSize: clampFontSize(settings.editor_font_size, DEFAULT_EDITOR_FONT_SIZE),
        chatFontSize: clampFontSize(settings.chat_font_size, DEFAULT_CHAT_FONT_SIZE),
    };
}

function getZoomScopeTarget(target: EventTarget | null): FontSizeTarget | null {
    if (!(target instanceof Element)) {
        return null;
    }

    const scopedElement = target.closest<HTMLElement>('[data-font-zoom-scope]');
    const scope = scopedElement?.dataset.fontZoomScope;
    return scope === 'editor' || scope === 'chat' ? scope : null;
}

export const DisplaySettingsProvider: React.FC<React.PropsWithChildren> = ({ children }) => {
    const bootstrapContext = useOptionalStartupBootstrap();
    const bootstrap = bootstrapContext?.bootstrap ?? null;
    const [editorFontSize, setEditorFontSizeState] = useState(DEFAULT_EDITOR_FONT_SIZE);
    const [chatFontSize, setChatFontSizeState] = useState(DEFAULT_CHAT_FONT_SIZE);
    const latestRemoteSettingsRef = useRef<RemoteAiConfig | null>(null);
    const latestFontSizesRef = useRef({
        editorFontSize: DEFAULT_EDITOR_FONT_SIZE,
        chatFontSize: DEFAULT_CHAT_FONT_SIZE,
    });
    const saveTimeoutRef = useRef<number | null>(null);

    const applySettings = useCallback((settings: RemoteAiConfig) => {
        latestRemoteSettingsRef.current = settings;
        const normalized = normalizeRemoteFontSettings(settings);
        latestFontSizesRef.current = normalized;
        setEditorFontSizeState(normalized.editorFontSize);
        setChatFontSizeState(normalized.chatFontSize);
    }, []);

    const loadSettings = useCallback(async () => {
        const settings = await invoke<RemoteAiConfig>('get_remote_ai_settings');
        applySettings(settings);
    }, [applySettings]);

    useEffect(() => {
        if (bootstrap) {
            applySettings(bootstrap.remote_settings);
            return;
        }

        void loadSettings().catch((error) => {
            console.error('[DisplaySettings] Failed to load display settings:', error);
        });
    }, [applySettings, bootstrap, loadSettings]);

    useEffect(() => {
        const unlistenPromise = listen('remote-settings-changed', () => {
            void loadSettings().catch((error) => {
                console.error('[DisplaySettings] Failed to reload display settings:', error);
            });
        });

        return () => {
            unlistenPromise.then((unlisten) => unlisten());
        };
    }, [loadSettings]);

    useEffect(() => {
        document.documentElement.style.setProperty('--editor-content-font-size', `${editorFontSize}px`);
        document.documentElement.style.setProperty('--chat-content-font-size', `${chatFontSize}px`);
        document.documentElement.style.setProperty('--markdown-font-size', `${editorFontSize}px`);
    }, [chatFontSize, editorFontSize]);

    const persistFontSizes = useCallback((nextFontSizes: { editorFontSize: number; chatFontSize: number }) => {
        if (saveTimeoutRef.current !== null) {
            window.clearTimeout(saveTimeoutRef.current);
        }

        saveTimeoutRef.current = window.setTimeout(() => {
            saveTimeoutRef.current = null;
            const remoteSettings = latestRemoteSettingsRef.current;
            if (!remoteSettings) {
                return;
            }

            const nextRemoteSettings: RemoteAiConfig = {
                ...remoteSettings,
                editor_font_size: nextFontSizes.editorFontSize,
                chat_font_size: nextFontSizes.chatFontSize,
            };

            latestRemoteSettingsRef.current = nextRemoteSettings;
            void invoke('save_remote_ai_settings', { settings: nextRemoteSettings })
                .then(() => emit('remote-settings-changed'))
                .catch((error) => {
                    console.error('[DisplaySettings] Failed to save font settings:', error);
                });
        }, 250);
    }, []);

    useEffect(() => {
        return () => {
            if (saveTimeoutRef.current !== null) {
                window.clearTimeout(saveTimeoutRef.current);
            }
        };
    }, []);

    const setFontSizeForTarget = useCallback((target: FontSizeTarget, fontSize: number) => {
        const fallback = target === 'editor' ? DEFAULT_EDITOR_FONT_SIZE : DEFAULT_CHAT_FONT_SIZE;
        const normalizedFontSize = clampFontSize(fontSize, fallback);
        const current = latestFontSizesRef.current;
        const nextFontSizes = target === 'editor'
            ? { ...current, editorFontSize: normalizedFontSize }
            : { ...current, chatFontSize: normalizedFontSize };

        if (
            nextFontSizes.editorFontSize === current.editorFontSize
            && nextFontSizes.chatFontSize === current.chatFontSize
        ) {
            return;
        }

        latestFontSizesRef.current = nextFontSizes;
        setEditorFontSizeState(nextFontSizes.editorFontSize);
        setChatFontSizeState(nextFontSizes.chatFontSize);
        persistFontSizes(nextFontSizes);
    }, [persistFontSizes]);

    const adjustFontSize = useCallback((target: FontSizeTarget, delta: number) => {
        const current = target === 'editor'
            ? latestFontSizesRef.current.editorFontSize
            : latestFontSizesRef.current.chatFontSize;
        setFontSizeForTarget(target, current + delta);
    }, [setFontSizeForTarget]);

    useEffect(() => {
        const handleWheel = (event: WheelEvent) => {
            if (!event.ctrlKey) {
                return;
            }

            const target = getZoomScopeTarget(event.target);
            if (!target) {
                return;
            }

            event.preventDefault();
            event.stopPropagation();
            adjustFontSize(target, event.deltaY < 0 ? 1 : -1);
        };

        window.addEventListener('wheel', handleWheel, { passive: false, capture: true });
        return () => window.removeEventListener('wheel', handleWheel, { capture: true });
    }, [adjustFontSize]);

    const value = useMemo<DisplaySettingsContextValue>(() => ({
        editorFontSize,
        chatFontSize,
        setEditorFontSize: (fontSize: number) => setFontSizeForTarget('editor', fontSize),
        setChatFontSize: (fontSize: number) => setFontSizeForTarget('chat', fontSize),
        adjustFontSize,
    }), [adjustFontSize, chatFontSize, editorFontSize, setFontSizeForTarget]);

    return (
        <DisplaySettingsContext.Provider value={value}>
            {children}
        </DisplaySettingsContext.Provider>
    );
};

export function useDisplaySettings() {
    const context = useContext(DisplaySettingsContext);
    if (!context) {
        throw new Error('useDisplaySettings must be used within a DisplaySettingsProvider');
    }
    return context;
}
