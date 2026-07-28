const DEFAULT_CHAT_VIRTUALIZATION_OVERSCAN_PX = 720;

export interface VisibleVirtualRange {
    startIndex: number;
    endIndex: number;
    topSpacerHeight: number;
    bottomSpacerHeight: number;
}

export function sameVisibleVirtualRange(a: VisibleVirtualRange, b: VisibleVirtualRange): boolean {
    return a.startIndex === b.startIndex
        && a.endIndex === b.endIndex
        && a.topSpacerHeight === b.topSpacerHeight
        && a.bottomSpacerHeight === b.bottomSpacerHeight;
}

/**
 * Smallest index in [lowerBound, rowCount] whose predicate holds, or rowCount
 * when none does. Row offsets are a running total of strictly positive row
 * heights, so both predicates used below are monotonic and a binary search
 * returns exactly what a forward linear scan would.
 */
function findFirstIndex(
    lowerBound: number,
    rowCount: number,
    predicate: (index: number) => boolean,
): number {
    let low = lowerBound;
    let high = rowCount;
    while (low < high) {
        const mid = (low + high) >>> 1;
        if (predicate(mid)) {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    return low;
}

export function computeVisibleVirtualRange(
    scrollTop: number,
    viewportHeight: number,
    virtualizedRowOffsets: number[],
    virtualizedRowHeights: number[],
    totalVirtualizedHeight: number,
    overscanPx = DEFAULT_CHAT_VIRTUALIZATION_OVERSCAN_PX,
): VisibleVirtualRange {
    const rowCount = virtualizedRowHeights.length;
    if (rowCount === 0) {
        return { startIndex: 0, endIndex: 0, topSpacerHeight: 0, bottomSpacerHeight: 0 };
    }

    const viewportStart = Math.max(0, scrollTop - overscanPx);
    const viewportEnd = scrollTop + viewportHeight + overscanPx;

    // This runs once per scroll frame, so it must not scan the whole timeline:
    // a long conversation would otherwise walk thousands of rows every rAF.
    const startIndex = findFirstIndex(0, rowCount, (index) => (
        virtualizedRowOffsets[index] + virtualizedRowHeights[index] >= viewportStart
    ));
    const endIndex = findFirstIndex(startIndex, rowCount, (index) => (
        virtualizedRowOffsets[index] >= viewportEnd
    ));

    // Offsets are a running total, so the height of rows [startIndex, endIndex)
    // is the difference between their offsets — no need to re-sum them.
    const topSpacerHeight = virtualizedRowOffsets[startIndex] ?? totalVirtualizedHeight;
    const renderedEndOffset = virtualizedRowOffsets[endIndex] ?? totalVirtualizedHeight;
    const bottomSpacerHeight = Math.max(0, totalVirtualizedHeight - renderedEndOffset);

    return { startIndex, endIndex, topSpacerHeight, bottomSpacerHeight };
}
