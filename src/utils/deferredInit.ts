import { recordDebugPerf } from './debugPerf';

export type DeferredInitPriority = 'soon' | 'idle' | 'background';

export interface DeferredInitOptions {
    label: string;
    priority?: DeferredInitPriority;
    timeoutMs?: number;
    signal?: AbortSignal;
}

export interface DeferredInitHandle {
    cancel: () => void;
}

type IdleDeadline = {
    didTimeout: boolean;
    timeRemaining: () => number;
};

type RequestIdleCallback = (
    callback: (deadline: IdleDeadline) => void,
    options?: { timeout?: number },
) => number;

type CancelIdleCallback = (handle: number) => void;

const DEFAULT_TIMEOUT_MS: Record<DeferredInitPriority, number> = {
    soon: 150,
    idle: 500,
    background: 1500,
};

function getIdleScheduler(): {
    requestIdleCallback?: RequestIdleCallback;
    cancelIdleCallback?: CancelIdleCallback;
} {
    const host = globalThis as typeof globalThis & {
        requestIdleCallback?: RequestIdleCallback;
        cancelIdleCallback?: CancelIdleCallback;
        window?: {
            requestIdleCallback?: RequestIdleCallback;
            cancelIdleCallback?: CancelIdleCallback;
        };
    };

    return {
        requestIdleCallback: host.requestIdleCallback ?? host.window?.requestIdleCallback,
        cancelIdleCallback: host.cancelIdleCallback ?? host.window?.cancelIdleCallback,
    };
}

function getTimeoutScheduler(): {
    setTimeout: typeof globalThis.setTimeout;
    clearTimeout: typeof globalThis.clearTimeout;
} {
    return {
        setTimeout: globalThis.setTimeout.bind(globalThis),
        clearTimeout: globalThis.clearTimeout.bind(globalThis),
    };
}

function timeoutForOptions(options: DeferredInitOptions): number {
    return options.timeoutMs ?? DEFAULT_TIMEOUT_MS[options.priority ?? 'idle'];
}

export function scheduleDeferredInit(
    task: () => void | Promise<void>,
    options: DeferredInitOptions,
): DeferredInitHandle {
    const timeoutMs = timeoutForOptions(options);
    const { signal } = options;

    if (signal?.aborted) {
        recordDebugPerf(`deferredInit.cancelled.${options.label}`);
        return { cancel: () => undefined };
    }

    let cancelled = false;
    let idleHandle: number | null = null;
    let timeoutHandle: ReturnType<typeof globalThis.setTimeout> | null = null;

    const cleanupAbortListener = () => {
        signal?.removeEventListener('abort', cancel);
    };

    const run = () => {
        if (cancelled || signal?.aborted) {
            recordDebugPerf(`deferredInit.cancelled.${options.label}`);
            cleanupAbortListener();
            return;
        }

        cancelled = true;
        cleanupAbortListener();
        recordDebugPerf(`deferredInit.run.${options.label}`);

        void Promise.resolve()
            .then(task)
            .then(() => {
                recordDebugPerf(`deferredInit.done.${options.label}`);
            })
            .catch((error) => {
                recordDebugPerf(`deferredInit.error.${options.label}`);
                console.error(`[deferredInit] ${options.label} failed:`, error);
            });
    };

    function cancel() {
        if (cancelled) {
            return;
        }

        cancelled = true;
        cleanupAbortListener();

        const { cancelIdleCallback } = getIdleScheduler();
        const { clearTimeout } = getTimeoutScheduler();

        if (idleHandle !== null && cancelIdleCallback) {
            cancelIdleCallback(idleHandle);
        }
        if (timeoutHandle !== null) {
            clearTimeout(timeoutHandle);
        }

        recordDebugPerf(`deferredInit.cancelled.${options.label}`);
    }

    recordDebugPerf(`deferredInit.schedule.${options.label}`);
    signal?.addEventListener('abort', cancel, { once: true });

    const { requestIdleCallback } = getIdleScheduler();
    if (requestIdleCallback) {
        idleHandle = requestIdleCallback(() => run(), { timeout: timeoutMs });
    } else {
        const { setTimeout } = getTimeoutScheduler();
        timeoutHandle = setTimeout(run, timeoutMs);
    }

    return { cancel };
}
