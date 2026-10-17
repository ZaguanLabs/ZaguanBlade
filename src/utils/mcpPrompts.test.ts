import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { McpPromptRender } from '../types/integrations';
import { insertionFromPrompt, insertionRefusal, isEmptyInsertion } from './mcpPrompts';
import { MAX_RESOURCE_ATTACHMENTS } from './mcpResources';

const labels = { user: 'User', assistant: 'Assistant' };
const render = (messages: McpPromptRender['messages']): McpPromptRender => ({ name: 'review', description: null, messages, truncated: false });

test('inserts user-only text unlabelled', () => {
    const insertion = insertionFromPrompt('Docs', render([
        { role: 'user', part: { kind: 'text', text: 'Review this.' } },
        { role: 'user', part: { kind: 'text', text: 'Be brief.' } },
    ]), labels);
    assert.equal(insertion.text, 'Review this.\n\nBe brief.');
});

test('labels every block when roles are mixed and never drops content silently', () => {
    const insertion = insertionFromPrompt('Docs', render([
        { role: 'user', part: { kind: 'text', text: 'Q' } },
        { role: 'assistant', part: { kind: 'text', text: 'A' } },
        { role: 'user', part: { kind: 'resource', uri: 'mem://dir/guide.md', mime_type: 'text/markdown', text: 'é' } },
        { role: 'user', part: { kind: 'image', mime_type: 'image/png', data: 'AAAA' } },
        { role: 'user', part: { kind: 'link', uri: 'mem://b', name: 'B' } },
        { role: 'user', part: { kind: 'unsupported' } },
    ]), labels);
    assert.equal(insertion.text, 'User:\nQ\n\nAssistant:\nA');
    assert.equal(insertion.resources.length, 1);
    assert.deepEqual([insertion.resources[0].server, insertion.resources[0].name, insertion.resources[0].bytes], ['Docs', 'guide.md', 2]);
    assert.equal(insertion.images[0].dataUrl, 'data:image/png;base64,AAAA');
    assert.equal(insertion.links, 1);
    assert.equal(insertion.unsupported, 1);
    assert.equal(isEmptyInsertion(insertion), false);
});

test('refuses the whole insertion when one resource would not fit', () => {
    const insertion = insertionFromPrompt('Docs', render([
        { role: 'user', part: { kind: 'resource', uri: 'mem://a', mime_type: null, text: 'a' } },
    ]), labels);
    assert.equal(insertionRefusal([], insertion), null);
    assert.equal(insertionRefusal(insertion.resources, insertion), 'duplicate');
    const full = Array.from({ length: MAX_RESOURCE_ATTACHMENTS }, (_, index) => ({ ...insertion.resources[0], uri: `mem://${index}x` }));
    assert.equal(insertionRefusal(full, insertion), 'count');
    assert.equal(isEmptyInsertion(insertionFromPrompt('Docs', render([{ role: 'user', part: { kind: 'text', text: ' ' } }]), labels)), true);
});
