import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import { parseMarkdownIntoBlocks } from 'streamdown';
import {
    createIncrementalMarkdownBlockParser,
    getStreamRenderIntervalMs,
} from './streamRendering';

describe('getStreamRenderIntervalMs', () => {
    test('keeps short streamed responses responsive without repainting every frame', () => {
        assert.equal(getStreamRenderIntervalMs(2_000, 1_000), 100);
    });

    test('reduces render pressure as accumulated Markdown grows', () => {
        assert.equal(getStreamRenderIntervalMs(10_000, 2_000), 160);
        assert.equal(getStreamRenderIntervalMs(40_000, 8_000), 250);
    });

    test('treats invalid negative lengths as empty input', () => {
        assert.equal(getStreamRenderIntervalMs(-1, -1), 100);
    });
});

describe('createIncrementalMarkdownBlockParser', () => {
    test('matches Streamdown while incomplete Markdown grows', () => {
        const parseIncrementally = createIncrementalMarkdownBlockParser(parseMarkdownIntoBlocks);
        const finalMarkdown = [
            '# Heading',
            '',
            'A paragraph with **bold text** and a [link](https://example.com).',
            '',
            '- first item',
            '- second item',
            '',
            '```ts',
            'const answer = 42;',
            '```',
        ].join('\n');

        for (let length = 1; length <= finalMarkdown.length; length += 3) {
            const markdown = finalMarkdown.slice(0, length);
            assert.deepEqual(parseIncrementally(markdown), parseMarkdownIntoBlocks(markdown));
        }
        assert.deepEqual(parseIncrementally(finalMarkdown), parseMarkdownIntoBlocks(finalMarkdown));
    });

    test('only reparses the mutable tail when completed blocks are unchanged', () => {
        const parsedInputs: string[] = [];
        const parseIncrementally = createIncrementalMarkdownBlockParser((markdown) => {
            parsedInputs.push(markdown);
            return parseMarkdownIntoBlocks(markdown);
        });

        parseIncrementally('Stable paragraph.\n\nGrowing');
        parseIncrementally('Stable paragraph.\n\nGrowing paragraph.');

        assert.equal(parsedInputs[0], 'Stable paragraph.\n\nGrowing');
        assert.equal(parsedInputs[1], 'Growing paragraph.');
    });

    test('falls back safely when an earlier block changes', () => {
        const parseIncrementally = createIncrementalMarkdownBlockParser(parseMarkdownIntoBlocks);
        parseIncrementally('First paragraph.\n\nSecond paragraph.');

        const edited = 'Edited first paragraph.\n\nSecond paragraph.';
        assert.deepEqual(parseIncrementally(edited), parseMarkdownIntoBlocks(edited));
    });
});
