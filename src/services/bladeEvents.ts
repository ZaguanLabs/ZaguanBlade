import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import i18n from '../i18n';
import type { BladeEvent, BladeEventEnvelope, BladeEventTransport } from '../types/blade';

type BladeEventListener = (envelope: BladeEventEnvelope) => void;
type Unsubscribe = () => void;
type BladeEventOf<TType extends BladeEvent['type']> = Extract<BladeEvent, { type: TType }>;
type BladeEventPayloadOf<TType extends BladeEvent['type']> = BladeEventOf<TType>['payload'];
type NestedBladeEventType<TType extends BladeEvent['type']> = BladeEventPayloadOf<TType> extends {
    type: infer TEventType extends string;
}
    ? TEventType
    : never;
type NestedBladeEventPayload<
    TType extends BladeEvent['type'],
    TEventType extends NestedBladeEventType<TType>,
> = Extract<BladeEventPayloadOf<TType>, { type: TEventType }> extends {
    payload: infer TPayload;
}
    ? TPayload
    : never;

const subscribers = new Map<number, BladeEventListener>();
let nextSubscriberId = 1;
let tauriUnlisten: UnlistenFn | null = null;
let listenerSetupPromise: Promise<void> | null = null;

function isTauriRuntime(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function normalizeBladeEventTransport(transport: BladeEventTransport): BladeEventEnvelope[] {
    return transport.transport === 'Batch' ? transport.payload : [transport.payload];
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

    listenerSetupPromise = listen<BladeEventTransport>('blade-event', (event) => {
        const envelopes = normalizeBladeEventTransport(event.payload);
        for (const envelope of envelopes) {
            for (const subscriber of subscribers.values()) {
                try {
                    subscriber(envelope);
                } catch (error) {
                    console.error('[bladeEvents] Subscriber failed:', error);
                }
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
    listener: (envelope: BladeEventEnvelope & { event: BladeEventOf<TType> }) => void,
): Unsubscribe {
    return subscribeBladeEvents((envelope) => {
        if (envelope.event.type !== type) {
            return;
        }

        listener(envelope as BladeEventEnvelope & { event: BladeEventOf<TType> });
    });
}

export function subscribeBladeNestedEventType<
    TType extends BladeEvent['type'],
    TEventType extends NestedBladeEventType<TType>,
>(
    type: TType,
    eventType: TEventType,
    listener: (
        payload: NestedBladeEventPayload<TType, TEventType>,
        envelope: BladeEventEnvelope & { event: BladeEventOf<TType> },
    ) => void,
): Unsubscribe {
    return subscribeBladeEventType(type, (envelope) => {
        const nestedEvent = envelope.event.payload;
        if (nestedEvent.type !== eventType) {
            return;
        }

        listener(
            nestedEvent.payload as NestedBladeEventPayload<TType, TEventType>,
            envelope,
        );
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
            reject(new Error(i18n.t('errors.bladeEventTimeout', {
                defaultValue: 'Timed out waiting for blade-event after {{timeoutMs}}ms',
                timeoutMs,
            })));
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
