import { useEffect } from 'react';
import { Routes, Route, Navigate } from 'react-router-dom';
import { listen } from '@tauri-apps/api/event';
import { AppLayout } from './components/Layout';
import { initNotifications, notifyFileChanges } from './utils/notifications';
import { useCoreStateSync } from './hooks/useCoreStateSync';

export default function App() {
    // Initialize core state sync for headless architecture
    const { isRecovering, coreState, featureFlags, error } = useCoreStateSync();

    useEffect(() => {
        if (coreState) {
            console.log('[App] Core state recovered:', {
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

    return (
        <Routes>
            <Route path="/" element={<AppLayout />} />
            <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
    );
}
