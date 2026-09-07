import assert from 'node:assert/strict';
import { test } from 'node:test';
import { IntegrationProbeRequest, integrationProbeErrorKey } from './integrationProbe';
import type { IntegrationLaunchReview, IntegrationProbeResult } from '../types/integrations';

const review: IntegrationLaunchReview = { ticket_id: 'ticket', executable: '/program', cwd: '/project', args: [] };
const result: IntegrationProbeResult = { protocol_version: '1', tools: 0, resources: false, prompts: false, authentication_methods: 0 };
function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>(done => { resolve = done; });
    return { promise, resolve };
}

test('closing the form cancels a launch review that arrives late', async () => {
    const pending = deferred<IntegrationLaunchReview>();
    const calls: string[] = [];
    const request = new IntegrationProbeRequest(async <T>(command: string) => {
        calls.push(command);
        return (command === 'prepare_integration_test' ? await pending.promise : undefined) as T;
    });
    const preparing = request.prepare('entry', 'revision', '/project');
    request.dispose();
    pending.resolve(review);
    assert.equal(await preparing, null);
    assert.deepEqual(calls, ['prepare_integration_test', 'cancel_integration_test']);
    assert.equal(await request.run(), null);
});

test('a test runs once and late results are ignored after cancellation', async () => {
    const pending = deferred<IntegrationProbeResult>();
    const calls: string[] = [];
    const request = new IntegrationProbeRequest(async <T>(command: string) => {
        calls.push(command);
        return (command === 'prepare_integration_test' ? review : command === 'run_integration_test' ? await pending.promise : undefined) as T;
    });
    await request.prepare('entry', 'revision', '/project');
    const running = request.run();
    assert.equal(await request.run(), null);
    request.dispose();
    pending.resolve(result);
    assert.equal(await running, null);
    assert.deepEqual(calls, ['prepare_integration_test', 'run_integration_test', 'cancel_integration_test']);
});

test('raw protocol and credential errors cannot become visible error messages', () => {
    assert.equal(integrationProbeErrorKey('secret_missing'), 'settings.integrations.test.errors.secret_missing');
    assert.equal(integrationProbeErrorKey('token=private-value'), 'settings.integrations.test.errors.protocol_failed');
    assert.equal(integrationProbeErrorKey(new Error('private-value')), 'settings.integrations.test.errors.protocol_failed');
});
