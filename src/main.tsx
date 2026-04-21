import React, { Suspense, useEffect } from 'react';
import ReactDOM from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import App from './App';
import { ErrorBoundary } from './components/ErrorBoundary';
import { ContextMenuProvider } from './components/ui/ContextMenu';
import { LanguageProvider } from './contexts/LanguageContext';
import { StartupBootstrapProvider, useStartupBootstrap } from './contexts/StartupBootstrapContext';
import { ThemeProvider } from './contexts/ThemeContext';
import './index.css';
import './i18n'; // Initialize i18n
import { parseBooleanFlag, readDebugFlag } from './utils/debugFlags';
import { startDebugPerfReporter } from './utils/debugPerf';

let hasHiddenLoadingScreen = false;
let hasTriggeredPostUiStartup = false;

window.__ZBLADE_DEBUG_FLAGS__ = {
    disableTERMINAL: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_TERMINAL),
    disableEDITOR: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_EDITOR),
    disableCHAT: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_CHAT),
    blankApp: parseBooleanFlag(import.meta.env.VITE_ZBLADE_BLANK_APP),
    disableCoreStateSync: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_CORE_STATE_SYNC),
    minimalLayout: parseBooleanFlag(import.meta.env.VITE_ZBLADE_MINIMAL_LAYOUT),
    disableChatHook: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_CHAT_HOOK),
    disableGitStatus: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_GIT_STATUS),
    disableUncommittedChanges: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_UNCOMMITTED_CHANGES),
    disableLayoutEvents: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_LAYOUT_EVENTS),
    disableProjectState: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_PROJECT_STATE),
    disableWarmup: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_WARMUP),
    disableTabManager: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_TAB_MANAGER),
    disableActivityBar: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_ACTIVITY_BAR),
    disableSidebarOverlay: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_SIDEBAR_OVERLAY),
    disableEditorChrome: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_EDITOR_CHROME),
    disableChatChrome: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_CHAT_CHROME),
    disableEditorWidthObserver: parseBooleanFlag(import.meta.env.VITE_ZBLADE_DISABLE_EDITOR_WIDTH_OBSERVER),
    debugPerf: undefined,
};

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

const DebugFlagBootstrap = ({ children }: { children: React.ReactNode }) => {
    const [ready, setReady] = React.useState(false);

    useEffect(() => {
        let cancelled = false;

        const loadRuntimeFlags = async () => {
            try {
                const runtimeFlags = await invoke<Record<string, string | undefined>>('get_runtime_debug_flags');
                if (!cancelled) {
                    window.__ZBLADE_DEBUG_FLAGS__ = {
                        ...window.__ZBLADE_DEBUG_FLAGS__,
                        ...runtimeFlags,
                    };
                    if (runtimeFlags.debugPerf === 'true') {
                        startDebugPerfReporter();
                    }
                }
            } catch (error) {
                console.error('[DEBUG] Failed to load runtime debug flags:', error);
            } finally {
                if (!cancelled) {
                    setReady(true);
                }
            }
        };

        void loadRuntimeFlags();

        return () => {
            cancelled = true;
        };
    }, []);

    if (!ready) {
        return <div className="h-screen w-screen bg-[var(--bg-app)]" />;
    }

    return <>{children}</>;
};

const AppWrapper = () => {
    const blankApp = readDebugFlag('blankApp');

    if (blankApp) {
        return (
            <div className="h-screen w-screen bg-[var(--bg-app)] text-[var(--fg-secondary)] flex items-center justify-center">
                Blank app mode enabled
            </div>
        );
    }

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
                        <DebugFlagBootstrap>
                            <AppWrapper />
                        </DebugFlagBootstrap>
                    </ContextMenuProvider>
                </ThemeProvider>
            </LanguageProvider>
        </ErrorBoundary>
    </StartupBootstrapProvider>
);
