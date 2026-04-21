import { invoke } from '@tauri-apps/api/core';
import { readDebugFlag } from './debugFlags';

type CounterMap = Map<string, number>;

declare global {
    interface Window {
        __ZBLADE_DEBUG_PERF_COUNTERS__?: CounterMap;
        __ZBLADE_DEBUG_PERF_LAST__?: CounterMap;
        __ZBLADE_DEBUG_PERF_TIMER__?: number;
    }
}

function getCounterMap(): CounterMap {
    if (!window.__ZBLADE_DEBUG_PERF_COUNTERS__) {
        window.__ZBLADE_DEBUG_PERF_COUNTERS__ = new Map<string, number>();
    }
    return window.__ZBLADE_DEBUG_PERF_COUNTERS__;
}

function getLastMap(): CounterMap {
    if (!window.__ZBLADE_DEBUG_PERF_LAST__) {
        window.__ZBLADE_DEBUG_PERF_LAST__ = new Map<string, number>();
    }
    return window.__ZBLADE_DEBUG_PERF_LAST__;
}

export function isDebugPerfEnabled(): boolean {
    return typeof window !== 'undefined' && readDebugFlag('debugPerf');
}

export function recordDebugPerf(name: string, delta: number = 1): void {
    if (!isDebugPerfEnabled()) {
        return;
    }

    const counters = getCounterMap();
    counters.set(name, (counters.get(name) ?? 0) + delta);
}

export function startDebugPerfReporter(): void {
    if (typeof window === 'undefined' || !isDebugPerfEnabled() || window.__ZBLADE_DEBUG_PERF_TIMER__) {
        return;
    }

    void invoke('log_frontend', { message: '[DEBUG_PERF] reporter-started' }).catch(() => {
        console.warn('[DEBUG_PERF] reporter-started');
    });

    window.__ZBLADE_DEBUG_PERF_TIMER__ = window.setInterval(() => {
        const counters = getCounterMap();
        const last = getLastMap();
        const deltas: Array<[string, number]> = [];

        counters.forEach((value, key) => {
            const previous = last.get(key) ?? 0;
            const delta = value - previous;
            if (delta > 0) {
                deltas.push([key, delta]);
            }
            last.set(key, value);
        });

        if (deltas.length === 0) {
            return;
        }

        deltas.sort((a, b) => b[1] - a[1]);
        const message = `[DEBUG_PERF] ${new Date().toISOString()} ${deltas
            .slice(0, 20)
            .map(([key, value]) => `${key}=${value}`)
            .join(' ')}`;

        void invoke('log_frontend', { message }).catch(() => {
            console.warn(message);
        });
    }, 2000);
}
