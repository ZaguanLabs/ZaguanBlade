import assert from 'node:assert/strict';
import { test } from 'node:test';
import { saveSettingsChanges, type SettingsPayload, type SettingsSaveServices } from './settingsPersistence';
import { projectSettingsFromBackend, projectSettingsToBackend } from './projectSettings';

function payload(): SettingsPayload {
    return {
        remote: { api_key: '', user_id: '', user_email: '', tier: '', theme: 'zaguan-dark', markdown_view: 'split', language: 'en', editor_font_size: 14, chat_font_size: 14 },
        local: { ollama_enabled: false, ollama_url: 'http://localhost:11434', ollama_cloud_enabled: false, ollama_cloud_api_key: '', openai_compat_enabled: false, openai_compat_url: '', hidden_local_models: [] },
        project: { storage: { mode: 'local', sync_metadata: true, cache: { enabled: true, max_size_mb: 100 } }, context: { max_tokens: 8000, compression: { enabled: true, model: 'remote' } }, privacy: { telemetry: false }, editor: {}, skills: { config: [] }, allow_gitignored_files: false, auto_approve_run_commands: false, warmup_context_prefetch: true, integrations: { disabled_ids: ['blocked-server'] } },
        integrations: { schema_version: 1, entries: [] },
    };
}

function recorder() {
    const calls: string[] = [];
    const services: SettingsSaveServices = {
        async invoke<T>(command: string) { calls.push(command); return { revision: 'saved-revision' } as T; },
        async emit(event) { calls.push(event); },
        async changeLanguage() {},
        integrationSaved(revision) { calls.push(revision); },
    };
    return { calls, services };
}

test('unchanged settings do not write and a project is required for workspace settings', async () => {
    const previous = payload();
    const next = payload();
    const { calls, services } = recorder();
    await saveSettingsChanges(next, previous, '/workspace', 'missing', services);
    assert.deepEqual(calls, []);
    next.project.auto_approve_run_commands = true;
    await saveSettingsChanges(next, previous, null, 'missing', services);
    assert.deepEqual(calls, []);
});

test('persistence failure rejects before success notifications and can be retried', async () => {
    const previous = payload();
    const next = payload();
    next.project.auto_approve_run_commands = true;
    const { calls, services } = recorder();
    const failure = new Error('disk full');
    await assert.rejects(saveSettingsChanges(next, previous, '/workspace', 'missing', {
        ...services, async invoke() { throw failure; },
    }), error => error === failure);
    assert.deepEqual(calls, []);
    await saveSettingsChanges(next, previous, '/workspace', 'missing', services);
    assert.deepEqual(calls, ['save_project_settings', 'project-settings-changed']);
});

test('a durable integration revision is retained even if a later notification fails', async () => {
    const previous = payload();
    const next = payload();
    next.remote.theme = 'another-theme';
    next.integrations.entries.push({ id: 'server', name: 'MCP', enabled: false, connection: { protocol: 'mcp', transport: { type: 'streamable_http', url: 'https://example.test/mcp', headers: {} } } });
    const { calls, services } = recorder();
    await assert.rejects(saveSettingsChanges(next, previous, '/workspace', 'missing', {
        ...services, async emit() { throw new Error('notification failed'); },
    }), /notification failed/);
    assert.deepEqual(calls, ['save_remote_ai_settings', 'save_integration_settings', 'saved-revision']);
});

test('workspace integration overrides survive an unrelated project preference change', async () => {
    const previous = payload();
    const next = payload();
    const form = projectSettingsFromBackend(previous.project);
    form.warmupContextPrefetch = false;
    next.project = projectSettingsToBackend(form);
    assert.deepEqual(next.project.integrations, { disabled_ids: ['blocked-server'] });
    const { services } = recorder();
    await saveSettingsChanges(next, previous, '/workspace', 'missing', {
        ...services,
        async invoke<T>(command: string, args: Record<string, unknown>) {
            assert.equal(command, 'save_project_settings');
            assert.deepEqual(args.settings, next.project);
            return undefined as T;
        },
    });
});

test('project preferences written before integration support get an empty override', () => {
    const old = payload().project;
    delete old.integrations;
    const restored = projectSettingsToBackend(projectSettingsFromBackend(old));
    assert.deepEqual(restored.integrations, { disabled_ids: [] });
    assert.equal(restored.warmup_context_prefetch, true);
});

test('changing only the theme does not trigger account or model reconciliation', async () => {
    const previous = payload();
    const next = payload();
    next.remote.theme = 'another-theme';
    const { calls, services } = recorder();
    await saveSettingsChanges(next, previous, '/workspace', 'missing', services);
    assert.deepEqual(calls, ['save_remote_ai_settings', 'theme-changed']);
});

test('disabling the global index is persisted before clearing a workspace disable override', async () => {
    const previous = payload();
    previous.project.integrations = { disabled_ids: [], symbols_index_enabled: false };
    const next = payload();
    next.integrations.symbols_index_enabled = false;
    next.project.integrations = { disabled_ids: [], symbols_index_enabled: null };
    const { calls, services } = recorder();
    await saveSettingsChanges(next, previous, '/workspace', 'revision', services);
    assert.ok(calls.indexOf('save_integration_settings') < calls.indexOf('save_project_settings'));
});

test('index overrides survive conversion and unrelated preference changes', () => {
    const original = payload().project;
    original.integrations = { disabled_ids: ['server'], symbols_index_enabled: false };
    const draft = projectSettingsFromBackend(original);
    draft.warmupContextPrefetch = false;
    assert.deepEqual(projectSettingsToBackend(draft).integrations, original.integrations);
});
