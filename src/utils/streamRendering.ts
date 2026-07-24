const NORMAL_STREAM_RENDER_INTERVAL_MS = 32;
const LONG_STREAM_RENDER_INTERVAL_MS = 50;
const VERY_LONG_STREAM_RENDER_INTERVAL_MS = 80;
const LONG_STREAM_CHARACTER_THRESHOLD = 12_000;
const VERY_LONG_STREAM_CHARACTER_THRESHOLD = 48_000;

/**
 * Re-render short responses at roughly 30 fps, then progressively reduce the
 * cadence as reparsing the accumulated Markdown becomes more expensive.
 * Completion is flushed separately and is never delayed by this interval.
 */
export function getStreamRenderIntervalMs(
    contentLength: number,
    reasoningLength: number,
): number {
    const accumulatedLength = Math.max(0, contentLength) + Math.max(0, reasoningLength);
    if (accumulatedLength >= VERY_LONG_STREAM_CHARACTER_THRESHOLD) {
        return VERY_LONG_STREAM_RENDER_INTERVAL_MS;
    }
    if (accumulatedLength >= LONG_STREAM_CHARACTER_THRESHOLD) {
        return LONG_STREAM_RENDER_INTERVAL_MS;
    }
    return NORMAL_STREAM_RENDER_INTERVAL_MS;
}
