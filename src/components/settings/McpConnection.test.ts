import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import en from '../../../public/locales/en/translation.json';
import es from '../../../public/locales/es/translation.json';
import { McpConnection } from './McpConnection';
import { newIntegrationProcess } from '../../utils/integrationSettings';
import type { IntegrationDefinition, McpConnectionStatus } from '../../types/integrations';
for (const [language, translation] of Object.entries({ en, es })) {
    test(`live connection controls show actual status and translations in ${language}`, async () => {
        const i18n = createInstance();
        await i18n.init({ lng: language, resources: { [language]: { translation } }, interpolation: { escapeValue: false } });
        const entry: IntegrationDefinition = { id: 'server', name: 'Atlas Scout', enabled: true, connection: { protocol: 'mcp', transport: { type: 'stdio', process: newIntegrationProcess('atlas-scout', ['mcp']) } } };
        const status: McpConnectionStatus = { integration_id: entry.id, connection_id: 'connection', workspace: { workspace_id: 'workspace', generation: 'generation' }, phase: 'connected', protocol_version: '2025-11-25', catalog_revision: 'catalog', tools: 14, error: null };
        const render = (value: McpConnectionStatus | undefined) => renderToStaticMarkup(createElement(I18nextProvider, { i18n }, createElement(McpConnection, { entry, revision: 'saved', workspacePath: '/workspace', canTest: true, live: { status: value, phase: 'ready', update: () => {} } })));
        const html = render(status);
        assert.ok(html.includes(translation.settings.integrations.connection.connected));
        assert.ok(html.includes(translation.settings.integrations.connection.disconnect));
        assert.ok(html.includes(translation.settings.integrations.connection.refresh));
        assert.ok(!html.includes('settings.integrations.'));
        assert.ok(render(undefined).includes(translation.settings.integrations.connection.connect));
        const failed = render({ ...status, phase: 'failed', error: 'integration_disabled' });
        assert.ok(failed.includes(translation.settings.integrations.test.errors.integration_disabled));
    });
}
