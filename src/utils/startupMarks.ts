import { invoke } from '@tauri-apps/api/core';
import { readDebugFlag } from './debugFlags';

/**
 * Record a startup mark in the browser performance timeline and, when the
 * backend bridge is available, mirror it to the Rust-side startup mark log so
 * benchmarks can collect a single timeline.
 */
export function markStartup(name: string): void {
    if (typeof performance !== 'undefined' && 'mark' in performance) {
        performance.mark(name);
    }

    if (typeof window === 'undefined') {
        return;
    }

    const frontendElapsedMs = Math.round(
        typeof performance !== 'undefined' ? performance.now() : Date.now() - (window as unknown as { __zbladeProcessStart?: number }).__zbladeProcessStart!
    );

    // Fire-and-forget: startup marks should never block the render path.
    invoke('record_startup_mark', { name, frontendElapsedMs }).catch(() => {
        // Best-effort; the backend may not be ready for very early marks.
    });
}

/**
 * Retrieve all recorded startup marks from the backend.  Useful for benchmark
 * scripts that need to persist the startup timeline.
 */
export async function getStartupMarks(): Promise<Array<{
    name: string;
    elapsed_ms: number;
    frontend_elapsed_ms?: number;
}>> {
    return invoke('get_startup_marks');
}

/**
 * Log a named duration for ad-hoc frontend profiling when `debugPerf` is on.
 */
export function measureStartup(label: string, fn: () => void): void {
    const start = performance.now();
    fn();
    const duration = performance.now() - start;
    if (readDebugFlag('debugPerf')) {
        console.log(`[STARTUP] ${label}: ${duration.toFixed(2)}ms`);
    }
}
