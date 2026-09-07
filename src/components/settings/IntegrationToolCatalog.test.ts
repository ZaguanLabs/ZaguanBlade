import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import en from '../../../public/locales/en/translation.json';
import es from '../../../public/locales/es/translation.json';
import { IntegrationToolCatalog } from './IntegrationToolCatalog';
import type { McpCatalog } from '../../types/integrations';

for (const [language, translation] of Object.entries({ en, es })) {
    test(`catalog inspection escapes server text and bounds rendered tools in ${language}`, async () => {
        const i18n = createInstance();
        await i18n.init({ lng: language, resources: { [language]: { translation } }, interpolation: { escapeValue: false } });
        const catalog: McpCatalog = { schema_version: 1, integration_id: 'server', revision: 'revision',
            tools: Array.from({ length: 21 }, (_, i) => ({ alias: `mcp_${i}`, definition: {
                name: `tool_${i}`, description: '<img src="https://invalid.example/track" onerror="bad()">',
                inputSchema: { type: 'object' }, icons: [{ src: 'https://invalid.example/icon.svg' }],
                annotations: { readOnlyHint: true },
            } })),
        };
        const render = (value: McpCatalog) => renderToStaticMarkup(createElement(I18nextProvider, { i18n }, createElement(IntegrationToolCatalog, { catalog: value, serverName: 'Atlas Scout' })));
        const html = render(catalog);
        assert.ok(html.includes(translation.settings.integrations.catalog.help));
        assert.ok(html.includes(translation.settings.integrations.catalog.search));
        assert.ok(html.includes('aria-expanded="false"'));
        assert.ok(html.includes('&lt;img'));
        assert.ok(!html.includes('<img'));
        assert.ok(!html.includes('icon.svg'));
        assert.ok(!html.includes('<pre'));
        assert.ok(!html.includes('tool_20'));
        assert.ok(!html.includes('settings.integrations.'));
        assert.ok(render({ ...catalog, tools: [] }).includes(translation.settings.integrations.catalog.empty));
    });
}
