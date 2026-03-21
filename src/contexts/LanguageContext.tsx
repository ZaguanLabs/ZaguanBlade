import React, { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import i18n, { normalizeAppLanguage } from '../i18n';
import { useOptionalStartupBootstrap } from './StartupBootstrapContext';
import type { RemoteAiConfig } from '../types/settings';

function applyConfiguredLanguage(settings: RemoteAiConfig) {
    const configuredLanguage = settings.language?.trim();
    if (!configuredLanguage) {
        return;
    }

    const nextLanguage = normalizeAppLanguage(configuredLanguage);
    if (i18n.resolvedLanguage !== nextLanguage) {
        void i18n.changeLanguage(nextLanguage);
    }
}

export const LanguageProvider: React.FC<React.PropsWithChildren> = ({ children }) => {
    const bootstrapContext = useOptionalStartupBootstrap();
    const bootstrap = bootstrapContext?.bootstrap ?? null;

    useEffect(() => {
        let mounted = true;

        const loadLanguageSettings = async () => {
            try {
                const settings = await invoke<RemoteAiConfig>('get_remote_ai_settings');
                if (mounted) {
                    applyConfiguredLanguage(settings);
                }
            } catch (error) {
                console.error('[Language] Failed to load language settings:', error);
            }
        };

        if (bootstrap) {
            applyConfiguredLanguage(bootstrap.remote_settings);
        } else {
            void loadLanguageSettings();
        }

        const unlistenPromise = listen('remote-settings-changed', () => {
            void loadLanguageSettings();
        });

        return () => {
            mounted = false;
            unlistenPromise.then((unlisten) => unlisten());
        };
    }, [bootstrap]);

    return <>{children}</>;
};
