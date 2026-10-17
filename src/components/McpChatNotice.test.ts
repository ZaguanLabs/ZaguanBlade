import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import en from '../../public/locales/en/translation.json';
import es from '../../public/locales/es/translation.json';
import type { IntegrationDefinition, McpConnectionStatus } from '../types/integrations';
import { newIntegrationProcess } from '../utils/integrationSettings';
import { mcpChatNotice, McpChatNoticeView } from './McpChatNotice';

const server = (id: string, name: string, enabled = true): IntegrationDefinition =>
    ({ id, name, enabled, connection: { protocol: 'mcp', transport: { type: 'stdio', process: newIntegrationProcess('server') } } });
const status = (id: string, phase: McpConnectionStatus['phase']): McpConnectionStatus => ({
    connection_id: `c-${id}`, integration_id: id, name: id, workspace: { workspace_id: 'w', generation: 'g' }, phase,
    protocol_version: null, catalog_revision: null, tools: 0, resources: false, prompts: false, error: null,
});
const agent: IntegrationDefinition = { id: 'agent', name: 'Agent', enabled: true, connection: { protocol: 'acp', process: newIntegrationProcess('agent'), mcp_servers: [] } };

test('local models are told which enabled servers are not connected', () => {
    const entries = [server('a', 'Bit Lantern'), server('b', 'Docs'), server('c', 'Off', false), agent];
    assert.deepEqual(mcpChatNotice(entries, [], 'ollama'), { kind: 'not_connected', names: ['Bit Lantern', 'Docs'] });
    assert.deepEqual(mcpChatNotice(entries, [status('a', 'connected'), status('b', 'failed')], 'openai-compat'), { kind: 'not_connected', names: ['Docs'] });
    assert.equal(mcpChatNotice(entries, [status('a', 'connected'), status('b', 'connecting')], 'ollama'), null);
    assert.equal(mcpChatNotice([agent, server('c', 'Off', false)], [], 'ollama'), null);
});

test('cloud models are told only when a connected server goes unused', () => {
    const entries = [server('a', 'Bit Lantern')];
    assert.equal(mcpChatNotice(entries, [], 'zaguan'), null);
    assert.equal(mcpChatNotice(entries, [], undefined), null);
    assert.deepEqual(mcpChatNotice(entries, [status('a', 'connected')], 'zaguan'), { kind: 'cloud_model' });
});

for (const [language, translation] of Object.entries({ en, es })) {
    test(`notices render translated in ${language}`, async () => {
        const i18n = createInstance();
        await i18n.init({ lng: language, resources: { [language]: { translation } }, interpolation: { escapeValue: false } });
        const render = (notice: Parameters<typeof McpChatNoticeView>[0]['notice']) => renderToStaticMarkup(createElement(I18nextProvider, { i18n },
            createElement(McpChatNoticeView, { notice, onOpenSettings: () => {}, onDismiss: () => {} })));
        const one = render({ kind: 'not_connected', names: ['Bit Lantern'] });
        assert.ok(one.includes(translation.mcpChat.notice.notConnected_one.replace('{{names}}', 'Bit Lantern').replace(/'/g, '&#x27;')));
        assert.ok(one.includes(translation.mcpChat.notice.openSettings));
        const two = render({ kind: 'not_connected', names: ['A', 'B'] });
        assert.ok(two.includes(translation.mcpChat.notice.notConnected_other.replace('{{names}}', 'A, B').replace(/'/g, '&#x27;')));
        const cloud = render({ kind: 'cloud_model' });
        assert.ok(cloud.includes(translation.mcpChat.notice.cloudModel.replace(/'/g, '&#x27;')));
        assert.ok(!(one + two + cloud).includes('mcpChat.'));
    });
}
