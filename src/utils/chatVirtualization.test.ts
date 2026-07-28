import assert from 'node:assert/strict';
import test from 'node:test';
import { computeVisibleVirtualRange, sameVisibleVirtualRange } from './chatVirtualization';

test('computeVisibleVirtualRange returns an empty range when there are no rows', () => {
    assert.deepEqual(
        computeVisibleVirtualRange(0, 400, [], [], 0),
        { startIndex: 0, endIndex: 0, topSpacerHeight: 0, bottomSpacerHeight: 0 },
    );
});

test('computeVisibleVirtualRange includes rows in the viewport plus overscan', () => {
    const heights = [100, 100, 100, 100, 100];
    const offsets = [0, 100, 200, 300, 400];

    assert.deepEqual(
        computeVisibleVirtualRange(150, 100, offsets, heights, 500, 50),
        { startIndex: 0, endIndex: 3, topSpacerHeight: 0, bottomSpacerHeight: 200 },
    );
});

test('computeVisibleVirtualRange creates top and bottom spacers around rendered rows', () => {
    const heights = [80, 120, 160, 200, 240];
    const offsets = [0, 80, 200, 360, 560];

    assert.deepEqual(
        computeVisibleVirtualRange(390, 100, offsets, heights, 800, 0),
        { startIndex: 3, endIndex: 4, topSpacerHeight: 360, bottomSpacerHeight: 240 },
    );
});

test('computeVisibleVirtualRange matches a linear scan across the whole timeline', () => {
    // The lookup is a binary search over the offsets running total. Pin it to the
    // straightforward linear-scan definition it replaced, over varied row heights
    // and every scroll position, so an off-by-one cannot slip through.
    const linearScanRange = (
        scrollTop: number,
        viewportHeight: number,
        offsets: number[],
        heights: number[],
        total: number,
        overscanPx: number,
    ) => {
        const rowCount = heights.length;
        const viewportStart = Math.max(0, scrollTop - overscanPx);
        const viewportEnd = scrollTop + viewportHeight + overscanPx;

        let startIndex = 0;
        while (startIndex < rowCount && offsets[startIndex] + heights[startIndex] < viewportStart) {
            startIndex += 1;
        }
        let endIndex = startIndex;
        while (endIndex < rowCount && offsets[endIndex] < viewportEnd) {
            endIndex += 1;
        }

        const topSpacerHeight = offsets[startIndex] ?? total;
        let renderedHeight = 0;
        for (let index = startIndex; index < endIndex; index += 1) {
            renderedHeight += heights[index] ?? 0;
        }
        return {
            startIndex,
            endIndex,
            topSpacerHeight,
            bottomSpacerHeight: Math.max(0, total - topSpacerHeight - renderedHeight),
        };
    };

    const heights = Array.from({ length: 200 }, (_, index) => 80 + ((index * 37) % 240));
    const offsets: number[] = [];
    let total = 0;
    for (const height of heights) {
        offsets.push(total);
        total += height;
    }

    for (const overscanPx of [0, 50, 720]) {
        for (let scrollTop = -200; scrollTop <= total + 400; scrollTop += 53) {
            assert.deepEqual(
                computeVisibleVirtualRange(scrollTop, 600, offsets, heights, total, overscanPx),
                linearScanRange(scrollTop, 600, offsets, heights, total, overscanPx),
                `mismatch at scrollTop=${scrollTop} overscan=${overscanPx}`,
            );
        }
    }
});

test('sameVisibleVirtualRange compares all range fields', () => {
    const range = { startIndex: 1, endIndex: 3, topSpacerHeight: 100, bottomSpacerHeight: 200 };

    assert.equal(sameVisibleVirtualRange(range, { ...range }), true);
    assert.equal(sameVisibleVirtualRange(range, { ...range, bottomSpacerHeight: 201 }), false);
});
