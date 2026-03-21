import React, { Suspense, useEffect } from 'react';
import ReactDOM from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import App from './App';
import { ErrorBoundary } from './components/ErrorBoundary';
import { ContextMenuProvider } from './components/ui/ContextMenu';
import { LanguageProvider } from './contexts/LanguageContext';
import { StartupBootstrapProvider, useStartupBootstrap } from './contexts/StartupBootstrapContext';
import { ThemeProvider } from './contexts/ThemeContext';
import './index.css';
import './i18n'; // Initialize i18n

let hasShownWindow = false;
let hasHiddenLoadingScreen = false;
let hasTriggeredPostUiStartup = false;

function hideLoadingScreen() {
    if (hasHiddenLoadingScreen) {
        return;
    }

    hasHiddenLoadingScreen = true;
    const loadingScreen = document.getElementById('loading-screen');
    if (!loadingScreen) return;

    loadingScreen.classList.add('loaded');
    const remove = () => loadingScreen.remove();
    loadingScreen.addEventListener('transitionend', remove, { once: true });
    window.setTimeout(remove, 220);
}

async function revealWindow(signalPostUiStartup: boolean) {
    try {
        if (!hasShownWindow && typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
            hasShownWindow = true;
            await getCurrentWindow().show();
        }
    } catch (err) {
        console.error('[WINDOW] Failed to show window:', err);
    } finally {
        hideLoadingScreen();

        if (signalPostUiStartup && !hasTriggeredPostUiStartup) {
            hasTriggeredPostUiStartup = true;
            window.setTimeout(() => {
                void invoke('frontend_shell_ready').catch((err) => {
                    console.error('[WINDOW] Failed to trigger post-UI startup:', err);
                });
            }, 0);
        }
    }
}

const StartupWindowController = () => {
    const { isLoading, error } = useStartupBootstrap();

    useEffect(() => {
        requestAnimationFrame(() => {
            void revealWindow(false);
        });

        const timeoutId = window.setTimeout(() => {
            console.warn('[WINDOW] Startup reveal fallback triggered before shell-ready');
            void revealWindow(false);
        }, 800);

        return () => window.clearTimeout(timeoutId);
    }, []);

    useEffect(() => {
        if (isLoading) {
            return;
        }

        void revealWindow(true);

        if (error) {
            console.error('[WINDOW] Bootstrap completed with error:', error);
        }
    }, [error, isLoading]);

    return null;
};

const AppWrapper = () => {

    return (
        <Suspense fallback={<div className="h-screen w-screen bg-(--bg-app)" />}>
            <App />
        </Suspense>
    );
};

ReactDOM.createRoot(document.getElementById('root')!).render(
    <StartupBootstrapProvider>
        <StartupWindowController />
        <ErrorBoundary>
            <LanguageProvider>
                <ThemeProvider>
                    <ContextMenuProvider>
                        <AppWrapper />
                    </ContextMenuProvider>
                </ThemeProvider>
            </LanguageProvider>
        </ErrorBoundary>
    </StartupBootstrapProvider>
);
