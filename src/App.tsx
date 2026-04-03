import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { AppLayout } from './components/Layout';
import { initNotifications, notifyFileChanges } from './utils/notifications';
import { useCoreStateSync } from './hooks/useCoreStateSync';

export default function App() {
    // Initialize core state sync for headless architecture
    const { isRecovering, coreState, featureFlags, error } = useCoreStateSync();

    useEffect(() => {
        if (coreState) {
            console.debug('[App] Core state recovered:', {
                workspace: coreState.workspace.path,
                activeFile: coreState.editor.active_file,
                openFiles: coreState.editor.open_files.length,
                capabilities: coreState.protocol.capabilities.length,
            });
        }
        if (error) {
            console.error('[App] Core state recovery failed:', error);
        }
    }, [coreState, error]);

    useEffect(() => {
        // Initialize notification system
        initNotifications();

        // Listen for file changes from backend
        const setupListener = async () => {
            const unlisten = await listen<{ count: number; paths: string[] }>(
                'file-changes-detected',
                async (event) => {
                    const fileNames = event.payload.paths.map(
                        (p) => p.split('/').pop() || p
                    );
                    await notifyFileChanges(event.payload.count, fileNames);
                }
            );

            return unlisten;
        };

        const unlistenPromise = setupListener();

        return () => {
            unlistenPromise.then((unlisten) => unlisten());
        };
    }, []);

    useEffect(() => {
        if (typeof window === 'undefined') {
            return;
        }

        let repaintFrame1: number | null = null;
        let repaintFrame2: number | null = null;
        let lastRepaintAt = 0;
        let unlistenFocusChanged: (() => void) | undefined;

        const runRepaintRecovery = () => {
            const now = Date.now();
            if (now - lastRepaintAt < 250) {
                return;
            }
            lastRepaintAt = now;

            const root = document.getElementById('root');
            if (!root) {
                window.dispatchEvent(new Event('resize'));
                return;
            }

            if (repaintFrame1 !== null) {
                cancelAnimationFrame(repaintFrame1);
                repaintFrame1 = null;
            }
            if (repaintFrame2 !== null) {
                cancelAnimationFrame(repaintFrame2);
                repaintFrame2 = null;
            }

            repaintFrame1 = requestAnimationFrame(() => {
                repaintFrame1 = null;
                const previousTransform = root.style.transform;
                const previousWillChange = root.style.willChange;

                root.style.willChange = 'transform';
                root.style.transform = previousTransform ? `${previousTransform} translateZ(0)` : 'translateZ(0)';
                void root.getBoundingClientRect();
                window.dispatchEvent(new Event('resize'));

                repaintFrame2 = requestAnimationFrame(() => {
                    repaintFrame2 = null;
                    root.style.transform = previousTransform;
                    root.style.willChange = previousWillChange;
                    window.dispatchEvent(new Event('resize'));
                });
            });
        };

        const handleVisibilityChange = () => {
            if (document.visibilityState === 'visible') {
                runRepaintRecovery();
            }
        };

        const handleFocus = () => {
            runRepaintRecovery();
        };

        document.addEventListener('visibilitychange', handleVisibilityChange);
        window.addEventListener('focus', handleFocus);
        window.addEventListener('pageshow', handleFocus);

        if ('__TAURI_INTERNALS__' in window) {
            const currentWindow = getCurrentWindow();
            currentWindow.onFocusChanged(({ payload: focused }) => {
                if (focused) {
                    runRepaintRecovery();
                }
            }).then((unlisten) => {
                unlistenFocusChanged = unlisten;
            }).catch(console.error);
        }

        return () => {
            if (repaintFrame1 !== null) {
                cancelAnimationFrame(repaintFrame1);
            }
            if (repaintFrame2 !== null) {
                cancelAnimationFrame(repaintFrame2);
            }
            document.removeEventListener('visibilitychange', handleVisibilityChange);
            window.removeEventListener('focus', handleFocus);
            window.removeEventListener('pageshow', handleFocus);
            if (unlistenFocusChanged) {
                unlistenFocusChanged();
            }
        };
    }, []);

    return <AppLayout />;
}
