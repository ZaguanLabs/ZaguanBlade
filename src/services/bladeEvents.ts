import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { BladeEvent, BladeEventEnvelope } from '../types/blade';

type BladeEventListener = (envelope: BladeEventEnvelope) => void;
type Unsubscribe = () => void;

const subscribers = new Map<number, BladeEventListener>();
let nextSubscriberId = 1;
let tauriUnlisten: UnlistenFn | null = null;
let listenerSetupPromise: Promise<void> | null = null;

function isTauriRuntime(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

async function ensureBladeEventListener(): Promise<void> {
    if (!isTauriRuntime()) {
        return;
    }

    if (tauriUnlisten) {
        return;
    }

    if (listenerSetupPromise) {
        return listenerSetupPromise;
    }

    listenerSetupPromise = listen<BladeEventEnvelope>('blade-event', (event) => {
        const envelope = event.payload;
        for (const subscriber of subscribers.values()) {
            try {
                subscriber(envelope);
            } catch (error) {
                console.error('[bladeEvents] Subscriber failed:', error);
            }
        }
    }).then((unlisten) => {
        tauriUnlisten = unlisten;
    }).finally(() => {
        listenerSetupPromise = null;
    });

    return listenerSetupPromise;
}

function teardownBladeEventListenerIfIdle(): void {
    if (subscribers.size > 0) {
        return;
    }

    if (listenerSetupPromise) {
        void listenerSetupPromise.then(() => {
            if (subscribers.size === 0 && tauriUnlisten) {
                tauriUnlisten();
                tauriUnlisten = null;
            }
        });
        return;
    }

    if (tauriUnlisten) {
        tauriUnlisten();
        tauriUnlisten = null;
    }
}

export function subscribeBladeEvents(listener: BladeEventListener): Unsubscribe {
    const subscriberId = nextSubscriberId;
    nextSubscriberId += 1;
    subscribers.set(subscriberId, listener);
    void ensureBladeEventListener().catch((error) => {
        console.error('[bladeEvents] Failed to initialize blade-event listener:', error);
    });

    return () => {
        subscribers.delete(subscriberId);
        teardownBladeEventListenerIfIdle();
    };
}

export function subscribeBladeEventType<TType extends BladeEvent['type']>(
    type: TType,
    listener: (envelope: BladeEventEnvelope & { event: Extract<BladeEvent, { type: TType }> }) => void,
): Unsubscribe {
    return subscribeBladeEvents((envelope) => {
        if (envelope.event.type !== type) {
            return;
        }

        listener(envelope as BladeEventEnvelope & { event: Extract<BladeEvent, { type: TType }> });
    });
}

export async function waitForBladeEvent(
    predicate: (envelope: BladeEventEnvelope) => boolean,
    timeoutMs = 1000,
): Promise<BladeEventEnvelope> {
    await ensureBladeEventListener();

    return new Promise<BladeEventEnvelope>((resolve, reject) => {
        const timeoutId = window.setTimeout(() => {
            unsubscribe();
            reject(new Error(`Timed out waiting for blade-event after ${timeoutMs}ms`));
        }, timeoutMs);

        const unsubscribe = subscribeBladeEvents((envelope) => {
            if (!predicate(envelope)) {
                return;
            }

            window.clearTimeout(timeoutId);
            unsubscribe();
            resolve(envelope);
        });
    });
}
