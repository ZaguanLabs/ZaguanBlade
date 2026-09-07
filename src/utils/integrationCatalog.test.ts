import assert from 'node:assert/strict';
import { test } from 'node:test';
import { catalogPage } from './integrationCatalog';
import type { McpCatalogTool } from '../types/integrations';

const tools: McpCatalogTool[] = Array.from({ length: 45 }, (_, i) => ({ alias: `mcp_${i}`, definition: {
    name: `tool_${i}`, title: i === 27 ? 'Symbol navigation' : undefined,
    description: i === 41 ? 'Find callers of a function' : undefined, inputSchema: {},
} }));

test('catalog search covers original name, title and description without altering routes', () => {
    for (const [query, index] of [[' TOOL_27 ', 27], ['SYMBOL', 27], ['callers', 41]] as const) {
        const page = catalogPage(tools, query, 0);
        assert.equal(page.total, 1);
        assert.equal(page.tools[0], tools[index]);
    }
    assert.equal(catalogPage(tools, 'mcp_27', 0).total, 0);
});

test('catalog pagination bounds rendering and clamps stale pages after filtering', () => {
    assert.equal(catalogPage(tools, '', 0).tools.length, 20);
    assert.equal(catalogPage(tools, '', 1).tools[0], tools[20]);
    const last = catalogPage(tools, '', 999);
    assert.equal(last.tools.length, 5);
    assert.equal(last.page, 2);
    assert.equal(last.pages, 3);
    assert.equal(catalogPage(tools, 'callers', 2).tools[0], tools[41]);
    assert.equal(catalogPage(tools, '', Number.NaN).page, 0);
    assert.equal(catalogPage(tools, '', -1).page, 0);
    assert.equal(catalogPage(tools, 'no-match', 0).total, 0);
    assert.deepEqual(catalogPage([], '', 0).tools, []);
});
