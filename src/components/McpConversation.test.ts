import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import en from '../../public/locales/en/translation.json';
import es from '../../public/locales/es/translation.json';
import { McpTurnView, type McpTurnState } from './McpTurnPanel';
import { McpToolResult, parseMcpResult } from './McpToolResult';

for (const [language, translation] of Object.entries({ en, es })) {
    test(`MCP approval is explicit and translated in ${language}`, async () => {
        const i18n = createInstance();
        await i18n.init({ lng: language, resources: { [language]: { translation } }, interpolation: { escapeValue: false } });
        const state: McpTurnState = {
            turn_id: 'turn', conversation_id: 'conversation', available: 1, limit: 32,
            excluded: [{ server_name: 'Atlas', tool_name: 'omitted', reason: 'schema' }], blocked: null,
            pending: [{ request_id: 'request', server_name: 'Atlas', tool_name: 'symbol_search', arguments: { query: '<script>bad()</script>' } }],
        };
        let decisions = 0;
        const html = renderToStaticMarkup(createElement(I18nextProvider, { i18n }, createElement(McpTurnView, { state, busy: false, error: null, respond: () => { decisions++; } })));
        assert.equal(decisions, 0);
        assert.ok(html.includes(translation.mcpChat.catalog_one.replace('{{count}}', '1').replace('{{limit}}', '32')));
        assert.ok(html.includes(translation.mcpChat.allow));
        assert.ok(html.includes(translation.mcpChat.deny));
        assert.ok(html.includes(translation.mcpChat.excluded.schema));
        assert.ok(html.includes('symbol_search'));
        assert.ok(html.includes('&lt;script&gt;'));
        assert.ok(!html.includes('<script>'));
        assert.ok(!html.includes('mcpChat.'));
    });
    test(`MCP rich results are attributed and never auto-fetch media in ${language}`, async () => {
        const i18n = createInstance();
        await i18n.init({ lng: language, resources: { [language]: { translation } }, interpolation: { escapeValue: false } });
        const raw = JSON.stringify({ mcp_result_version: 1, server: 'Atlas', tool: 'symbol_search', result: { outcome: 'completed', projection: { text: '<img src="https://external/image">', truncated: true, non_text_blocks: 1, is_error: false }, artifact: { conversation_id: 'conversation', turn_id: 'turn', artifact_id: 'artifact' } } });
        const html = renderToStaticMarkup(createElement(I18nextProvider, { i18n }, createElement(McpToolResult, { alias: 'mcp_alias', raw, status: 'complete' })));
        assert.ok(html.includes('Atlas / symbol_search'));
        assert.ok(html.includes(translation.mcpChat.truncated));
        assert.ok(html.includes(translation.mcpChat.fullResult));
        assert.ok(html.includes('&lt;img'));
        assert.ok(!html.includes('<img') && !html.includes('<a '));
        assert.ok(!html.includes('mcpChat.'));
        const unknown = JSON.stringify({ mcp_result_version: 1, server: 'Atlas', tool: 'search', result: { outcome: 'unknown', error: 'cancelled', retry: false } });
        const cancelled = renderToStaticMarkup(createElement(I18nextProvider, { i18n }, createElement(McpToolResult, { alias: 'mcp_alias', raw: unknown, status: 'error' })));
        assert.ok(cancelled.includes(translation.mcpChat.outcome.unknown));
    });
}
test('malformed external result fields do not reach React children', () => {
    assert.equal(parseMcpResult('{'), null);
    assert.equal(parseMcpResult(JSON.stringify({ mcp_result_version: 1, server: {}, result: { outcome: 'completed' } })), null);
    assert.equal(parseMcpResult(JSON.stringify({ mcp_result_version: 1, result: { outcome: 'completed', projection: { text: {} } } })), null);
});
