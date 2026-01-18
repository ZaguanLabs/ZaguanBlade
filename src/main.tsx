import React, { Suspense, useEffect } from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { getCurrentWindow } from '@tauri-apps/api/window';
import App from './App';
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
        // Show window after React has hydrated and initial render is complete
        const showWindow = async () => {
            console.log('[WINDOW] Attempting to show window...');
            try {
                // Check if we're in Tauri environment
                if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
                    const appWindow = getCurrentWindow();
                    await appWindow.show();
                    console.log('[WINDOW] Window shown successfully');
                } else {
                    console.log('[WINDOW] Not in Tauri environment, skipping window.show()');
                }
                
                // Remove loading screen after window is shown
                setTimeout(() => {
                    const loadingScreen = document.getElementById('loading-screen');
                    if (loadingScreen) {
                        loadingScreen.classList.add('loaded');
                        setTimeout(() => loadingScreen.remove(), 300);
                    }
                }, 100);
            } catch (err) {
                console.error('[WINDOW] Failed to show window:', err);
                // Remove loading screen anyway to prevent permanent black screen
                const loadingScreen = document.getElementById('loading-screen');
                if (loadingScreen) {
                    loadingScreen.classList.add('loaded');
                    setTimeout(() => loadingScreen.remove(), 300);
                }
            }
        };

        // Small delay to ensure first paint is complete
        requestAnimationFrame(() => {
            requestAnimationFrame(() => {
                showWindow();
            });
        });
    }, []);

    return (
        <Suspense fallback={<div className="h-screen w-screen bg-[var(--bg-app)]" />}>
            <App />
        </Suspense>
    );
};

ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
        <BrowserRouter>
            <ContextMenuProvider>
                <AppWrapper />
            </ContextMenuProvider>
        </BrowserRouter>
    </React.StrictMode>
);
