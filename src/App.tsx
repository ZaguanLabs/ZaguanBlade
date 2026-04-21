import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { AppLayout } from './components/Layout';
import { initNotifications, notifyFileChanges } from './utils/notifications';
import { useCoreStateSync } from './hooks/useCoreStateSync';

function readDebugFlag(name: string): boolean {
    if (typeof window === 'undefined') {
        return false;
    }

    try {
        const globalFlags = (globalThis as { __ZBLADE_DEBUG_FLAGS__?: Record<string, unknown> }).__ZBLADE_DEBUG_FLAGS__;
        const envValue = globalFlags?.[name];
        if (envValue === true || envValue === '1' || envValue === 'true') {
            return true;
        }
        if (envValue === false || envValue === '0' || envValue === 'false') {
            return false;
        }
    } catch {
        // ignore
    }

    const queryValue = new URLSearchParams(window.location.search).get(name);
    if (queryValue === '1' || queryValue === 'true') {
        return true;
    }
    if (queryValue === '0' || queryValue === 'false') {
        return false;
    }

    try {
        const storageValue = window.localStorage.getItem(`zblade.debug.${name}`);
        return storageValue === '1' || storageValue === 'true';
    } catch {
        return false;
    }
}

function AppWithCoreStateSync() {
    const { isRecovering, coreState, featureFlags, error } = useCoreStateSync();
    void isRecovering;
    void featureFlags;

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

    return <AppLayout />;
}

function AppWithoutCoreStateSync() {
    useEffect(() => {
        initNotifications();

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

    return <AppLayout />;
}

export default function App() {
    const disableCoreStateSync = readDebugFlag('disableCoreStateSync');
    return disableCoreStateSync ? <AppWithoutCoreStateSync /> : <AppWithCoreStateSync />;
}
