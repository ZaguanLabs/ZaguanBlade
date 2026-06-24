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

test('sameVisibleVirtualRange compares all range fields', () => {
    const range = { startIndex: 1, endIndex: 3, topSpacerHeight: 100, bottomSpacerHeight: 200 };

    assert.equal(sameVisibleVirtualRange(range, { ...range }), true);
    assert.equal(sameVisibleVirtualRange(range, { ...range, bottomSpacerHeight: 201 }), false);
});
