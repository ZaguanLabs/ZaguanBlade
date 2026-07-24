import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import { getStreamRenderIntervalMs } from './streamRendering';

describe('getStreamRenderIntervalMs', () => {
    test('keeps short streamed responses responsive', () => {
        assert.equal(getStreamRenderIntervalMs(2_000, 1_000), 32);
    });

    test('reduces render pressure as accumulated Markdown grows', () => {
        assert.equal(getStreamRenderIntervalMs(10_000, 2_000), 50);
        assert.equal(getStreamRenderIntervalMs(40_000, 8_000), 80);
    });

    test('treats invalid negative lengths as empty input', () => {
        assert.equal(getStreamRenderIntervalMs(-1, -1), 32);
    });
});
