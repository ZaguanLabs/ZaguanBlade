import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import en from '../../../public/locales/en/translation.json';
import es from '../../../public/locales/es/translation.json';
import { LaunchReviewDetails } from './IntegrationLaunchReview';
import { IntegrationSettings } from './IntegrationSettings';
import { validHeaderName } from './IntegrationEnvironment';
import type { IntegrationConfig, IntegrationLaunchReview } from '../../types/integrations';

for (const [language, translation] of Object.entries({ en, es })) {
    const setup = async () => {
        const i18n = createInstance();
        await i18n.init({ lng: language, resources: { [language]: { translation } }, interpolation: { escapeValue: false } });
        return i18n;
    };

    test(`HTTP approvals show the endpoint and header names, never values, in ${language}`, async () => {
        const i18n = await setup();
        const render = (review: IntegrationLaunchReview, purpose: 'test' | 'connection') =>
            renderToStaticMarkup(createElement(I18nextProvider, { i18n }, createElement(LaunchReviewDetails, { review, purpose })));
        const http: IntegrationLaunchReview = { ticket_id: 'ticket', kind: 'http', url: 'https://tools.example/mcp', header_names: ['Authorization', 'X-Client'], insecure: false };
        const secure = render(http, 'connection');
        assert.ok(secure.includes('https://tools.example/mcp'));
        assert.ok(secure.includes('Authorization\nX-Client'));
        assert.ok(secure.includes(translation.settings.integrations.connection.reviewHelpHttp));
        assert.ok(!secure.includes(translation.settings.integrations.review.insecure));
        const insecure = render({ ...http, url: 'http://192.168.1.4/mcp', header_names: [], insecure: true }, 'test');
        assert.ok(insecure.includes(translation.settings.integrations.review.insecure));
        assert.ok(insecure.includes(translation.settings.integrations.review.noHeaders));
        assert.ok(insecure.includes(translation.settings.integrations.test.reviewHelpHttp));
        const process = render({ ticket_id: 'ticket', kind: 'process', executable: '/usr/bin/atlas-scout', cwd: '/workspace', args: ['mcp'] }, 'test');
        assert.ok(process.includes('/usr/bin/atlas-scout'));
        assert.ok(process.includes(translation.settings.integrations.test.reviewHelp));
        for (const html of [secure, insecure, process]) assert.ok(!html.includes('settings.integrations.'));
    });

    test(`HTTP servers can be tested and connected from Settings in ${language}`, async () => {
        const i18n = await setup();
        const config: IntegrationConfig = { schema_version: 1, entries: [{ id: 'remote', name: 'Remote', enabled: true, connection: { protocol: 'mcp',
            transport: { type: 'streamable_http', url: 'https://tools.example/mcp', headers: { Authorization: { kind: 'secret', name: 'authorization' } } } } }] };
        const html = renderToStaticMarkup(createElement(I18nextProvider, { i18n }, createElement(IntegrationSettings, { config, onChange: () => {} })));
        assert.ok(html.includes('value="https://tools.example/mcp"'));
        assert.ok(html.includes(translation.settings.integrations.headers.title));
        assert.ok(html.includes('Authorization'));
        assert.ok(html.includes(translation.settings.integrations.test.button));
        assert.ok(html.includes(translation.settings.integrations.connection.title));
        assert.ok(html.includes(i18n.t('settings.integrations.environment.credential', { name: 'authorization' })));
        assert.ok(!html.includes('settings.integrations.'));
    });
}

test('header names exclude transport-owned and invalid names', () => {
    for (const name of ['Authorization', 'X-Api-Key', 'cookie']) assert.ok(validHeaderName(name), name);
    for (const name of ['', 'Accept', 'MCP-Session-Id', 'Mcp-Param-Path', 'Bad Header', 'Line\nBreak', 'x'.repeat(129)]) assert.ok(!validHeaderName(name), name);
});
