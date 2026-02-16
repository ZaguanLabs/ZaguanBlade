import React, { Suspense, useEffect } from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { getCurrentWindow } from '@tauri-apps/api/window';
import App from './App';
import { ErrorBoundary } from './components/ErrorBoundary';
import { ContextMenuProvider } from './components/ui/ContextMenu';
import './index.css';
import './i18n'; // Initialize i18n
import "@fontsource/fira-code"; // Defaults to weight 400
import "@fontsource/fira-code/500.css"; // Medium
import "@fontsource/fira-code/600.css"; // Semi-bold
import "@fontsource/fira-code/700.css"; // Bold

// Wrapper to handle window visibility
const AppWrapper = () => {
    useEffect(() => {
        const hideLoadingScreen = () => {
            const loadingScreen = document.getElementById('loading-screen');
            if (!loadingScreen) return;

            loadingScreen.classList.add('loaded');
            const remove = () => loadingScreen.remove();
            loadingScreen.addEventListener('transitionend', remove, { once: true });
            window.setTimeout(remove, 220);
        };

        // Show window after first frame to reduce black/white flashes on startup
        requestAnimationFrame(() => {
            void (async () => {
                try {
                    if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
                        const appWindow = getCurrentWindow();
                        await appWindow.show();
                    }
                } catch (err) {
                    console.error('[WINDOW] Failed to show window:', err);
                } finally {
                    hideLoadingScreen();
                }
            })();
        });
    }, []);

    return (
        <Suspense fallback={<div className="h-screen w-screen bg-[var(--bg-app)]" />}>
            <App />
        </Suspense>
    );
};

ReactDOM.createRoot(document.getElementById('root')!).render(
    <ErrorBoundary>
        <BrowserRouter>
            <ContextMenuProvider>
                <AppWrapper />
            </ContextMenuProvider>
        </BrowserRouter>
    </ErrorBoundary>
);
