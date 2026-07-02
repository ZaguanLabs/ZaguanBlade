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

    let startIndex = 0;
    while (
        startIndex < rowCount
        && virtualizedRowOffsets[startIndex] + virtualizedRowHeights[startIndex] < viewportStart
    ) {
        startIndex += 1;
    }

    let endIndex = startIndex;
    while (endIndex < rowCount && virtualizedRowOffsets[endIndex] < viewportEnd) {
        endIndex += 1;
    }

    const topSpacerHeight = virtualizedRowOffsets[startIndex] ?? totalVirtualizedHeight;
    let renderedHeight = 0;
    for (let index = startIndex; index < endIndex; index += 1) {
        renderedHeight += virtualizedRowHeights[index] ?? 0;
    }
    const bottomSpacerHeight = Math.max(0, totalVirtualizedHeight - topSpacerHeight - renderedHeight);

    return { startIndex, endIndex, topSpacerHeight, bottomSpacerHeight };
}
