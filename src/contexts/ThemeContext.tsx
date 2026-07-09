import React, { createContext, useContext, useEffect, useLayoutEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useOptionalStartupBootstrap } from './StartupBootstrapContext';
import type { RemoteAiConfig } from '../types/settings';
import { applyThemeToDocument, availableThemes, defaultThemeId, getThemeById, normalizeThemeId } from '../themes';

interface ThemeContextValue {
    availableThemes: typeof availableThemes;
    currentTheme: ReturnType<typeof getThemeById>;
    themeId: string;
    setThemeId: (themeId: string) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

export const ThemeProvider: React.FC<React.PropsWithChildren> = ({ children }) => {
    const [themeId, setThemeIdState] = useState(defaultThemeId);
    const bootstrapContext = useOptionalStartupBootstrap();
    const bootstrap = bootstrapContext?.bootstrap ?? null;

    useEffect(() => {
        let mounted = true;

        const applyThemeSettings = (settings: RemoteAiConfig) => {
            if (mounted) {
                setThemeIdState(normalizeThemeId(settings.theme));
            }
        };

        const loadTheme = async () => {
            try {
                const settings = await invoke<RemoteAiConfig>('get_remote_ai_settings');
                applyThemeSettings(settings);
            } catch (error) {
                console.error('[Theme] Failed to load theme settings:', error);
                if (mounted) {
                    setThemeIdState(defaultThemeId);
                }
            }
        };

        if (bootstrap) {
            applyThemeSettings(bootstrap.remote_settings);
        } else {
            void loadTheme();
        }

        const unlistenPromise = listen('theme-changed', () => {
            void loadTheme();
        });

        return () => {
            mounted = false;
            unlistenPromise.then((unlisten) => unlisten());
        };
    }, [bootstrap]);

    // Use useLayoutEffect so the document CSS variables are updated synchronously
    // before the browser paints and before any child passive effects (e.g. the
    // Terminal component reading getComputedStyle) run. With useEffect, child
    // effects fire before parent effects, so the Terminal would read stale theme
    // tokens on theme change.
    useLayoutEffect(() => {
        applyThemeToDocument(themeId);
    }, [themeId]);

    const value = useMemo<ThemeContextValue>(() => ({
        availableThemes,
        currentTheme: getThemeById(themeId),
        themeId,
        setThemeId: (nextThemeId: string) => {
            setThemeIdState(normalizeThemeId(nextThemeId));
        },
    }), [themeId]);

    return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
};

export function useTheme() {
    const context = useContext(ThemeContext);
    if (!context) {
        throw new Error('useTheme must be used within a ThemeProvider');
    }
    return context;
}
