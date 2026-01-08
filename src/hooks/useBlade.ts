import { useCallback, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { BladeDispatcher } from '../services/blade';
import type { BladeIntent, SystemEvent, BladeError } from '../types/blade';
import { useToast } from './useToast'; // Assuming a toast hook exists, or we'll mock it for now

export function useBlade() {
    // We can integrate a toast notification system here for global error handling
    // const { toast } = useToast(); 

    useEffect(() => {
        let unlisten: (() => void) | undefined;

        const setupListener = async () => {
            unlisten = await listen<SystemEvent>('sys-event', (event) => {
                const payload = event.payload;

                // Handle System Events globally
                if (payload.type === 'IntentFailed') {
                    console.error('[BladeProtocol] Intent Failed:', payload.payload.error);
                    // TODO: Show toast
                    // toast({ title: "Error", description: formatError(payload.payload.error), variant: "destructive" });
                } else if (payload.type === 'ProcessStarted') {
                    console.log('[BladeProtocol] Process Started:', payload.payload.intent_id);
                }
            });
        };

        setupListener();

        return () => {
            if (unlisten) unlisten();
        };
    }, []);

    const dispatch = useCallback(async (domain: string, intent: BladeIntent) => {
        return BladeDispatcher.dispatch(domain, intent);
    }, []);

    const chat = useCallback(async (payload: BladeIntent extends { type: 'Chat' } ? BladeIntent['payload'] : never) => {
        return BladeDispatcher.chat(payload);
    }, []);

    const editor = useCallback(async (payload: BladeIntent extends { type: 'Editor' } ? BladeIntent['payload'] : never) => {
        return BladeDispatcher.editor(payload);
    }, []);

    return {
        dispatch,
        chat,
        editor
    };
}
