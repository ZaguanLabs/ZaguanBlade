import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import en from '../../public/locales/en/translation.json';
import es from '../../public/locales/es/translation.json';
import { AttachedResourcesDisplay } from '../components/ChatMessage';
import { McpResourcePicker } from '../chat/composer/McpResourcePicker';
import { attachRefusal, MAX_RESOURCE_ATTACHMENTS, MAX_RESOURCE_TEXT_BYTES, selectionFromRead, utf8Bytes } from './mcpResources';
import type { McpResourceRead } from '../types/integrations';

const read: McpResourceRead = {
    uri: 'docs://guide',
    truncated: true,
    parts: [
        { kind: 'text', uri: 'docs://guide/a', mime_type: 'text/markdown', text: 'Ä first' },
        { kind: 'text', uri: 'docs://guide/b', mime_type: 'text/markdown', text: 'second' },
        { kind: 'image', uri: 'docs://guide/logo.png', mime_type: 'image/png', data: 'iVBORw0KGgo=' },
        { kind: 'unsupported', uri: 'docs://guide/archive', mime_type: 'application/zip' },
    ],
};

test('a read becomes one text attachment, image attachments and an unsupported count', () => {
    const selection = selectionFromRead('Docs', 'Guide', read);
    const attachment = selection.attachment!;
    assert.equal(attachment.text, '--- docs://guide/a ---\nÄ first\n\n--- docs://guide/b ---\nsecond');
    assert.equal(attachment.bytes, utf8Bytes(attachment.text));
    assert.ok(attachment.bytes > attachment.text.length);
    assert.equal(attachment.truncated, true);
    assert.deepEqual(selection.images, [{ dataUrl: 'data:image/png;base64,iVBORw0KGgo=', name: 'logo.png' }]);
    assert.equal(selection.unsupported, 1);
    const single = selectionFromRead('Docs', 'Guide', { ...read, parts: [read.parts[1]] });
    assert.equal(single.attachment!.text, 'second');
    assert.equal(selectionFromRead('Docs', 'Guide', { ...read, parts: [read.parts[3]] }).attachment, null);
});

test('attachments are refused when duplicated, too many or too large', () => {
    const item = (uri: string, bytes = 10) => ({ ...selectionFromRead('Docs', 'Guide', { ...read, uri, parts: [read.parts[1]] }).attachment!, bytes });
    assert.equal(attachRefusal([], item('a')), null);
    assert.equal(attachRefusal([item('a')], item('a')), 'duplicate');
    assert.equal(attachRefusal(Array.from({ length: MAX_RESOURCE_ATTACHMENTS }, (_, index) => item(`r${index}`)), item('z')), 'count');
    assert.equal(attachRefusal([item('a', MAX_RESOURCE_TEXT_BYTES)], item('b', 1)), 'size');
});

for (const [language, translation] of Object.entries({ en, es })) {
    test(`resource provenance and picker render translated in ${language}`, async () => {
        const i18n = createInstance();
        await i18n.init({ lng: language, resources: { [language]: { translation } }, interpolation: { escapeValue: false } });
        const wrap = (element: ReturnType<typeof createElement>) => renderToStaticMarkup(createElement(I18nextProvider, { i18n }, element));
        const shown = wrap(createElement(AttachedResourcesDisplay, { resources: [{ server: 'Docs', name: 'Guide', uri: 'docs://guide', mime_type: 'text/markdown', bytes: 12, truncated: true }] }));
        assert.ok(shown.includes(translation.chat.mcpResources.attached));
        assert.ok(shown.includes('Docs: Guide'));
        assert.ok(shown.includes(translation.chat.mcpResources.truncated));
        const picker = wrap(createElement(McpResourcePicker, { onAttach: () => undefined, onClose: () => undefined }));
        assert.ok(picker.includes(translation.chat.mcpResources.title));
        assert.ok(picker.includes(translation.chat.mcpResources.help));
        for (const html of [shown, picker]) assert.ok(!html.includes('chat.mcpResources.'));
    });
}
