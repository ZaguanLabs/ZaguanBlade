import { useCallback, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { BladeDispatcher } from '../services/blade';
import type { BladeIntent, SystemEvent, BladeError, BladeEventEnvelope } from '../types/blade';
import { useToast } from './useToast'; // Assuming a toast hook exists, or we'll mock it for now

export function useBlade() {
    // We can integrate a toast notification system here for global error handling
    // const { toast } = useToast(); 

    useEffect(() => {
        let unlistenLegacy: (() => void) | undefined;
        let unlistenV11: (() => void) | undefined;

        const setupListeners = async () => {
            // Legacy sys-event listener (v1.0)
            unlistenLegacy = await listen<SystemEvent>('sys-event', (event) => {
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

            // v1.1: blade-event listener for new events
            unlistenV11 = await listen<BladeEventEnvelope>('blade-event', (event) => {
                const envelope = event.payload;
                
                if (envelope.event.type === 'System') {
                    const systemEvent = envelope.event.payload;
                    
                    if (systemEvent.type === 'ProtocolVersion') {
                        const { current, supported } = systemEvent.payload;
                        console.log(`[BladeProtocol v1.1] Server version: ${current.major}.${current.minor}.${current.patch}`);
                        console.log('[BladeProtocol v1.1] Supported versions:', supported);
                        
                        // Check compatibility
                        if (current.major !== 1) {
                            console.warn('[BladeProtocol v1.1] Version mismatch detected!');
                            // TODO: Show warning toast
                        }
                    } else if (systemEvent.type === 'ProcessProgress') {
                        const { intent_id, progress, message } = systemEvent.payload;
                        console.log(`[BladeProtocol v1.1] Progress [${intent_id}]: ${(progress * 100).toFixed(0)}% - ${message}`);
                        // TODO: Update progress UI
                    } else if (systemEvent.type === 'IntentFailed') {
                        console.error('[BladeProtocol v1.1] Intent Failed:', systemEvent.payload.error);
                    } else if (systemEvent.type === 'ProcessStarted') {
                        console.log('[BladeProtocol v1.1] Process Started:', systemEvent.payload.intent_id);
                    } else if (systemEvent.type === 'ProcessCompleted') {
                        console.log('[BladeProtocol v1.1] Process Completed:', systemEvent.payload.intent_id);
                    }
                }
            });
        };

        setupListeners();

        return () => {
            if (unlistenLegacy) unlistenLegacy();
            if (unlistenV11) unlistenV11();
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
