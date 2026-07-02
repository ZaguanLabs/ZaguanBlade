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
import { DisplaySettingsProvider } from './contexts/DisplaySettingsContext';
import './index.css';
import './i18n'; // Initialize i18n
import { parseBooleanFlag, readDebugFlag } from './utils/debugFlags';
import { startDebugPerfReporter } from './utils/debugPerf';

let hasHiddenLoadingScreen = false;
let hasShownWindow = false;
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

// NVIDIA/X11 WebKitGTK recovery (safety net). On NVIDIA/X11 the GPU context can be
// lost after the display sleeps for a long time, leaving a stale/black surface when
// the window is resumed. Forcing a full webview repaint when the window regains
// focus / becomes visible re-paints the layer and recovers the view. Most useful
// when GPU compositing is enabled (ZBLADE_WEBKIT_KEEP_COMPOSITING=1 — see
// src-tauri/src/linux_webkit_workaround.rs); harmless otherwise. Best-effort: only
// helps if the view is black-but-alive (events still flow); a hard GPU hang needs
// the default (compositing disabled). At most one imperceptible repaint per focus.
function forceWebviewRepaint() {
    try {
        const root = document.documentElement;
        // A sub-1 opacity promotes the root to its own compositor layer and marks
        // it dirty; holding it for a couple of frames then reverting forces a fresh
        // paint with no layout change and no visible flicker.
        root.style.opacity = '0.9999';
        requestAnimationFrame(() => {
            requestAnimationFrame(() => {
                root.style.opacity = '';
            });
        });
    } catch {
        // Document not ready / unavailable — nothing to recover.
    }
}

if (typeof window !== 'undefined') {
    window.addEventListener('focus', forceWebviewRepaint);
    window.addEventListener('pageshow', forceWebviewRepaint);
    document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'visible') {
            forceWebviewRepaint();
        }
    });
}

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

    if (!hasShownWindow) {
        hasShownWindow = true;
        try {
            await getCurrentWindow().show();
        } catch (err) {
            hasShownWindow = false;
            console.error('[WINDOW] Failed to show main window:', err);
        }
    }

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
                    <DisplaySettingsProvider>
                        <ContextMenuProvider>
                            <DebugFlagBootstrap>
                                <AppWrapper />
                            </DebugFlagBootstrap>
                        </ContextMenuProvider>
                    </DisplaySettingsProvider>
                </ThemeProvider>
            </LanguageProvider>
        </ErrorBoundary>
    </StartupBootstrapProvider>
);
