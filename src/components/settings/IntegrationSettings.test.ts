import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import en from '../../../public/locales/en/translation.json';
import es from '../../../public/locales/es/translation.json';
import { IntegrationSettings } from './IntegrationSettings';
import type { IntegrationConfig } from '../../types/integrations';
import { newIntegrationProcess } from '../../utils/integrationSettings';

for (const [language, translation] of Object.entries({ en, es })) {
    test(`integration forms render translated and accessible controls in ${language}`, async () => {
        const i18n = createInstance();
        await i18n.init({ lng: language, resources: { [language]: { translation } }, interpolation: { escapeValue: false } });
        const config: IntegrationConfig = { schema_version: 1, entries: [
            { id: 'server', name: 'Atlas Scout', enabled: false, connection: { protocol: 'mcp', transport: { type: 'stdio', process: newIntegrationProcess('atlas-scout', ['mcp', 'path with spaces']) } } },
            { id: 'agent', name: 'Agent', enabled: false, connection: { protocol: 'acp', process: { ...newIntegrationProcess('agent'), env: { API_KEY: { kind: 'secret', name: 'provider-key' } } }, mcp_servers: ['server'] } },
        ] };
        const html = renderToStaticMarkup(createElement(I18nextProvider, { i18n }, createElement(IntegrationSettings, { config, onChange: () => {} })));
        assert.ok(html.includes(translation.settings.integrations.notConnected));
        assert.ok(html.includes(translation.settings.integrations.description));
        assert.ok(html.includes('value="atlas-scout"'));
        assert.ok(html.includes('value="path with spaces"'));
        assert.ok(html.includes(`aria-label="${i18n.t('settings.integrations.argumentNumber', { number: 1 })}"`));
        assert.ok(html.includes('type="checkbox" checked=""'));
        assert.ok(html.includes('type="password"'));
        assert.ok(html.includes(translation.settings.integrations.test.button));
        assert.ok(html.includes(translation.settings.integrations.environment.credentials));
        assert.ok(!html.includes('settings.integrations.'));
    });
}
