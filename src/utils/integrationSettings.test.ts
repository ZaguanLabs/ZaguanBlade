import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { integrationErrorKey, newIntegrationProcess, removeIntegration, replaceIntegration } from './integrationSettings';
import type { IntegrationConfig } from '../types/integrations';

function fixture(): IntegrationConfig {
    return { schema_version: 1, entries: [
        { id: 'mcp-id', name: 'Atlas Scout', enabled: false, connection: { protocol: 'mcp', transport: { type: 'stdio', process: newIntegrationProcess('atlas-scout', ['mcp']) } } },
        { id: 'acp-id', name: 'Agent', enabled: false, connection: { protocol: 'acp', process: newIntegrationProcess('agent'), mcp_servers: ['mcp-id'] } },
    ] };
}

describe('integration configuration edits', () => {
    test('renaming preserves IDs, forwarding and fields outside the edited form', () => {
        const original = fixture();
        const next = replaceIntegration(original, { ...original.entries[0], name: 'Renamed' });
        assert.equal(next.entries[0].id, 'mcp-id');
        assert.deepEqual(next.entries[1], original.entries[1]);
        assert.equal(original.entries[0].name, 'Atlas Scout');
    });

    test('removing an MCP definition clears only its ACP forwarding references', () => {
        const original = fixture();
        const next = removeIntegration(original, 'mcp-id');
        assert.equal(next.entries.length, 1);
        assert.equal(next.entries[0].connection.protocol, 'acp');
        if (next.entries[0].connection.protocol === 'acp') assert.deepEqual(next.entries[0].connection.mcp_servers, []);
        assert.equal(original.entries.length, 2);
    });

    test('unexpected errors cannot expose a URL, command argument or secret', () => {
        assert.equal(integrationErrorKey('conflict'), 'settings.integrations.errors.conflict');
        assert.equal(integrationErrorKey('https://example.test/?token=private'), 'settings.integrations.errors.unknown');
        assert.equal(integrationErrorKey({ secret: 'private' }), 'settings.integrations.errors.unknown');
    });
});
