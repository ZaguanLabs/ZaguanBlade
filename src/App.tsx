import { useEffect } from 'react';
import { Routes, Route, Navigate } from 'react-router-dom';
import { listen } from '@tauri-apps/api/event';
import { AppLayout } from './components/Layout';
import { initNotifications, notifyFileChanges } from './utils/notifications';

export default function App() {
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
