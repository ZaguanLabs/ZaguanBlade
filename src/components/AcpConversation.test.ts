import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import en from '../../public/locales/en/translation.json';
import es from '../../public/locales/es/translation.json';
import type { AcpAgentStatus } from '../types/acp';
import { addPrompt, applyAcpEvent, startAgentChat, storedTurns, type AgentChat } from '../utils/acpConversation';
import { AcpConversationView } from './AcpConversation';
import { enabledAgents } from './AcpAgentMenu';

const status = (ready: boolean): AcpAgentStatus => ({
    conversation_id: 'c1', integration_id: 'agent', name: 'Fixture Agent', ready,
    snapshot: {
        protocol_version: 1, agent: { name: 'fixture', title: null, version: '1.0' }, load_session: false, resume_session: false,
        close_session: false, list_sessions: false, logout: false, prompt_image: false, prompt_audio: false,
        prompt_embedded_context: false, mcp_http: false, mcp_sse: false,
        auth_methods: [{ id: 'login', name: 'Log in with Fixture', description: null, terminal: false },
            { id: 'tty', name: 'Terminal login', description: null, terminal: true }],
    },
});
const noop = () => {};
const escape = (text: string) => text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#x27;');

for (const [language, translation] of Object.entries({ en, es })) {
    const render = async (chat: AgentChat) => {
        const i18n = createInstance();
        await i18n.init({ lng: language, resources: { [language]: { translation } }, interpolation: { escapeValue: false } });
        return renderToStaticMarkup(createElement(I18nextProvider, { i18n }, createElement(AcpConversationView, {
            chat, busy: false, error: null, send: noop, cancel: noop, answer: noop, authenticate: noop, configure: noop, end: noop, onOpenFile: noop,
        })));
    };
    const strings = translation.chat.agent;

    test(`a permission request offers exactly the agent's options and reports stay display-only in ${language}`, async () => {
        let chat = addPrompt(startAgentChat(status(true)), 1, 'Fix the bug');
        chat = applyAcpEvent(chat, { type: 'tool_call', turn: 1, call: {
            id: 'c', title: 'Edit main.rs', kind: 'edit', status: 'in_progress', truncated: false,
            locations: [{ path: '/w/src/main.rs', line: 4 }],
            content: [{ type: 'diff', path: '/w/src/main.rs', old_text: 'a', new_text: 'b' }],
        } });
        chat = applyAcpEvent(chat, { type: 'permission', turn: 1, request: {
            request_id: 'r1', tool_call: { id: 'c', title: 'Edit main.rs', kind: 'edit', status: 'pending', locations: [], content: [], truncated: false },
            options: [{ id: 'yes', name: 'Allow this edit', kind: 'allow_once' }, { id: 'never', name: 'Never for this file', kind: 'reject_always' }],
        } });
        const html = await render(chat);
        assert.ok(html.includes(escape(strings.reportsHelp)));
        assert.ok(html.includes('Allow this edit') && html.includes('Never for this file'));
        assert.ok(html.includes(escape(strings.permission.dismiss)));
        assert.ok(html.includes(escape(strings.tool.status.in_progress)));
        assert.ok(html.includes('/w/src/main.rs:4'));
        assert.ok(html.includes(escape(strings.working)));
        assert.ok(html.includes(escape(strings.stop)));
    });

    test(`sign-in offers only the agent's own methods and an exited session cannot be messaged in ${language}`, async () => {
        const signIn = await render(startAgentChat(status(false)));
        assert.ok(signIn.includes('Log in with Fixture'));
        assert.ok(signIn.includes(escape(strings.signIn.terminal.replace('{{method}}', 'Terminal login'))));
        assert.ok(!signIn.includes('>Terminal login</button>'));
        let chat = addPrompt(startAgentChat(status(true)), 1, 'Go');
        chat = applyAcpEvent(chat, { type: 'exited', error: 'agent_exited' });
        const exited = await render(chat);
        assert.ok(exited.includes(escape(translation.settings.integrations.test.errors.agent_exited)));
        assert.ok(/<textarea[^>]*disabled/.test(exited));
        assert.ok(!exited.includes(escape(strings.end)));
    });

    test(`files the agent wrote through Blade are listed and refusals are explained in ${language}`, async () => {
        let chat = addPrompt(startAgentChat(status(true)), 1, 'Edit');
        chat = applyAcpEvent(chat, { type: 'file_refused', turn: 1, path: '/w/src/lib.rs', reason: 'unsaved_changes' });
        chat = applyAcpEvent(chat, { type: 'file_written', turn: 1, path: '/w/src/main.rs' });
        chat = applyAcpEvent(chat, { type: 'file_written', turn: 1, path: '/w/src/main.rs' });
        chat = applyAcpEvent(chat, { type: 'file_refused', turn: 1, path: '/w/.git/config', reason: 'protected' });
        assert.deepEqual(chat.turns[0].files.map(file => file.path), ['/w/src/lib.rs', '/w/src/main.rs', '/w/.git/config']);
        const html = await render(chat);
        assert.ok(html.includes(escape(strings.files.written)));
        assert.equal(html.split('>/w/src/main.rs</button>').length, 2);
        assert.ok(html.includes(escape(strings.files.refused.unsaved_changes.replace('{{path}}', '/w/src/lib.rs'))));
        assert.ok(html.includes(escape(strings.files.refused.protected.replace('{{path}}', '/w/.git/config'))));
    });

    test(`terminals show once, inside the tool call that runs them, with their status in ${language}`, async () => {
        let chat = addPrompt(startAgentChat(status(true)), 1, 'Build');
        const terminal = (id: string, command: string, exit: { exit_code: number | null; signal: string | null } | null) =>
            ({ id, command, cwd: '/w', output: '\u001b[32mok\u001b[0m\n', truncated: exit === null, exit, released: false });
        chat = applyAcpEvent(chat, { type: 'terminal', turn: 1, terminal: terminal('t1', 'cargo build', null) });
        chat = applyAcpEvent(chat, { type: 'tool_call', turn: 1, call: {
            id: 'c', title: 'Build', kind: 'execute', status: 'in_progress', truncated: false, locations: [],
            content: [{ type: 'terminal', terminal_id: 't1' }],
        } });
        chat = applyAcpEvent(chat, { type: 'terminal', turn: 1, terminal: terminal('t2', 'npm test', { exit_code: 1, signal: null }) });
        chat = applyAcpEvent(chat, { type: 'terminal', turn: 1, terminal: terminal('t3', 'sleep 30', { exit_code: null, signal: 'SIGKILL' }) });
        assert.deepEqual(chat.turns[0].terminals, ['t1', 't2', 't3']);
        const html = await render(chat);
        assert.equal(html.split('$ cargo build').length, 2, 'shown once');
        assert.ok(html.includes(escape(strings.terminal.running)) && html.includes(escape(strings.terminal.truncated)));
        assert.ok(html.includes(escape(strings.terminal.exited.replace('{{code}}', '1'))));
        assert.ok(html.includes(escape(strings.terminal.signal.replace('{{signal}}', 'SIGKILL'))));
        assert.ok(html.includes('>ok</pre>') && !html.includes('[32m'));
    });

    test(`the agent's settings show as its own controls and lock during a turn in ${language}`, async () => {
        let chat = applyAcpEvent(startAgentChat(status(true)), { type: 'config_changed', config: { modes: null, options: [
            { id: 'model', name: 'Model', description: null, category: 'model', type: 'select', current_value: 'a/1', groups: [
                { name: 'Provider A', choices: [{ value: 'a/1', name: 'Model One', description: null }] },
                { name: 'Provider B', choices: [{ value: 'b/2', name: 'Model Two', description: 'Fast' }] }] },
            { id: 'fast', name: 'Fast mode', description: null, category: null, type: 'boolean', current_value: true },
        ] } });
        const idle = await render(chat);
        assert.ok(idle.includes(escape(strings.config.label.replace('{{name}}', 'Fixture Agent'))));
        assert.ok(idle.includes('<optgroup label="Provider B"><option value="b/2" title="Fast">Model Two</option></optgroup>'));
        assert.ok(/<option value="a\/1" selected="">Model One<\/option>/.test(idle));
        assert.ok(/<input type="checkbox" checked=""\/><span>Fast mode<\/span>/.test(idle));
        assert.ok(!idle.includes(`<span>${escape(strings.config.mode)}</span>`));
        chat = addPrompt(chat, 1, 'Go');
        const running = await render(chat);
        assert.ok(running.includes(escape(strings.config.duringTurn)));
        assert.ok(/<select[^>]*disabled=""/.test(running) && /<input type="checkbox" disabled="" checked=""/.test(running));

        const legacy = await render(applyAcpEvent(startAgentChat(status(true)), { type: 'config_changed', config: { options: [],
            modes: { current_mode_id: 'ask', modes: [{ value: 'ask', name: 'Ask', description: null }, { value: 'code', name: 'Code', description: null }] } } }));
        assert.ok(legacy.includes(`<span>${escape(strings.config.mode)}</span>`));
        assert.ok(legacy.includes('<option value="code">Code</option>'));
    });

    test(`a reconnected session says whether the agent remembers the stored turns in ${language}`, async () => {
        const history = storedTurns([{ role: 'User', content: 'Earlier question' }, { role: 'Assistant', content: 'Earlier answer' }]);
        const notice = (key: 'resumed' | 'replaced') => escape(strings.reopened[key].replace('{{name}}', 'Fixture Agent'));
        const resumed = await render(addPrompt(startAgentChat({ ...status(true), reopened: 'resumed' }, history), 1, 'Next question'));
        assert.ok(resumed.includes(notice('resumed')));
        assert.ok(resumed.indexOf('Earlier answer') < resumed.indexOf(notice('resumed')));
        assert.ok(resumed.indexOf(notice('resumed')) < resumed.indexOf('Next question'));
        const replaced = await render(startAgentChat({ ...status(true), reopened: 'replaced' }, history));
        assert.ok(replaced.includes(notice('replaced')));
        assert.ok(!replaced.includes(escape(strings.working)), 'stored turns are finished');
        const fresh = await render(startAgentChat({ ...status(true), reopened: 'new' }));
        assert.ok(!Object.values(strings.reopened).some(text => fresh.includes(escape(text.replace('{{name}}', 'Fixture Agent')))));
    });
}

test('only enabled ACP agents are offered', () => {
    const process = { command: 'agent', args: [], cwd: null, env: {} };
    const agents = enabledAgents({ revision: 'r', config: { schema_version: 1, entries: [
        { id: 'a', name: 'On', enabled: true, connection: { protocol: 'acp', process, mcp_servers: [] } },
        { id: 'b', name: 'Off', enabled: false, connection: { protocol: 'acp', process, mcp_servers: [] } },
        { id: 'c', name: 'Server', enabled: true, connection: { protocol: 'mcp', transport: { type: 'stdio', process } } },
    ] } });
    assert.deepEqual(agents.map(agent => agent.id), ['a']);
});
