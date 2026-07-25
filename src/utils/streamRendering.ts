const NORMAL_STREAM_RENDER_INTERVAL_MS = 100;
const LONG_STREAM_RENDER_INTERVAL_MS = 160;
const VERY_LONG_STREAM_RENDER_INTERVAL_MS = 250;
const LONG_STREAM_CHARACTER_THRESHOLD = 12_000;
const VERY_LONG_STREAM_CHARACTER_THRESHOLD = 48_000;

/**
 * Re-render short responses at 10 fps, then progressively reduce the cadence
 * as reparsing and laying out the accumulated Markdown becomes more expensive.
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

type MarkdownBlockParser = (markdown: string) => string[];

/**
 * Streamdown keeps completed blocks mounted, but its default block splitter
 * still tokenizes the entire accumulated response on every chunk. Preserve the
 * verified stable prefix and only split the mutable tail of an append-heavy
 * stream. If remend or an edit changes an earlier block, progressively back off
 * to a matching boundary and finally to a full parse.
 */
export function createIncrementalMarkdownBlockParser(
    parseMarkdownIntoBlocks: MarkdownBlockParser,
): MarkdownBlockParser {
    let previousBlocks: string[] = [];
    let stableBlockCount = 0;
    let stablePrefix = '';

    const updateCache = (blocks: string[], retainedStableBlockCount = 0) => {
        const nextStableBlockCount = Math.max(0, blocks.length - 1);
        if (
            retainedStableBlockCount > 0
            && nextStableBlockCount >= retainedStableBlockCount
            && retainedStableBlockCount === stableBlockCount
        ) {
            stablePrefix += blocks
                .slice(retainedStableBlockCount, nextStableBlockCount)
                .join('');
        } else {
            stablePrefix = blocks.slice(0, nextStableBlockCount).join('');
        }
        stableBlockCount = nextStableBlockCount;
        previousBlocks = blocks;
    };

    return (markdown: string) => {
        if (stableBlockCount > 0 && markdown.startsWith(stablePrefix)) {
            const tailBlocks = parseMarkdownIntoBlocks(markdown.slice(stablePrefix.length));
            const blocks = [
                ...previousBlocks.slice(0, stableBlockCount),
                ...tailBlocks,
            ];
            updateCache(blocks, stableBlockCount);
            return blocks;
        }

        // An incomplete construct can occasionally make remend revise more
        // than the final block. Reuse the longest earlier boundary that still
        // exactly prefixes the new input.
        for (let retainedBlockCount = stableBlockCount - 1; retainedBlockCount > 0; retainedBlockCount -= 1) {
            const prefix = previousBlocks.slice(0, retainedBlockCount).join('');
            if (!markdown.startsWith(prefix)) {
                continue;
            }

            const blocks = [
                ...previousBlocks.slice(0, retainedBlockCount),
                ...parseMarkdownIntoBlocks(markdown.slice(prefix.length)),
            ];
            stableBlockCount = retainedBlockCount;
            stablePrefix = prefix;
            updateCache(blocks, retainedBlockCount);
            return blocks;
        }

        const blocks = parseMarkdownIntoBlocks(markdown);
        stableBlockCount = 0;
        stablePrefix = '';
        updateCache(blocks);
        return blocks;
    };
}
