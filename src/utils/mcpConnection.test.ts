import assert from 'node:assert/strict';
import { test } from 'node:test';
import { McpConnectionRequest } from './mcpConnection';
import type { IntegrationLaunchReview, McpConnectionStatus } from '../types/integrations';
const review: IntegrationLaunchReview = { ticket_id: 'ticket', executable: '/server', cwd: '/workspace', args: [] };
const status: McpConnectionStatus = { connection_id: 'ticket', integration_id: 'server', workspace: { workspace_id: 'workspace', generation: 'generation' }, phase: 'connecting', protocol_version: null, catalog_revision: null, tools: 0, error: null };
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }

test('late prepare is cancelled and never becomes launchable', async () => {
    const pending = deferred<IntegrationLaunchReview>();
    const calls: string[] = [];
    const request = new McpConnectionRequest(async <T>(command: string) => { calls.push(command); return (command === 'prepare_mcp_connection' ? await pending.promise : undefined) as T; });
    const preparing = request.prepare('server', 'revision', '/workspace');
    request.dispose(); pending.resolve(review);
    assert.equal(await preparing, null);
    assert.equal(await request.connect(), null);
    assert.deepEqual(calls, ['prepare_mcp_connection', 'cancel_integration_test']);
});

test('cancellation during connect also cancels a late accepted connection', async () => {
    const pending = deferred<McpConnectionStatus>();
    const calls: string[] = [];
    const request = new McpConnectionRequest(async <T>(command: string) => {
        calls.push(command); return (command === 'prepare_mcp_connection' ? review : command === 'connect_mcp' ? await pending.promise : undefined) as T;
    });
    await request.prepare('server', 'revision', '/workspace');
    const connecting = request.connect();
    assert.equal(await request.connect(), null);
    request.dispose(); pending.resolve(status);
    assert.equal(await connecting, null);
    assert.deepEqual(calls, ['prepare_mcp_connection', 'connect_mcp', 'cancel_integration_test', 'cancel_integration_test']);
});

test('closing Settings preserves an accepted connection and cannot launch it twice', async () => {
    const calls: string[] = [];
    const request = new McpConnectionRequest(async <T>(command: string) => { calls.push(command); return (command === 'prepare_mcp_connection' ? review : status) as T; });
    await request.prepare('server', 'revision', '/workspace');
    assert.equal(await request.connect(), status);
    assert.equal(await request.connect(), null);
    request.dispose();
    assert.deepEqual(calls, ['prepare_mcp_connection', 'connect_mcp']);
});
