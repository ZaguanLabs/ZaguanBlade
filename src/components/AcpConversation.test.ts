import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import en from '../../public/locales/en/translation.json';
import es from '../../public/locales/es/translation.json';
import type { AcpAgentStatus } from '../types/acp';
import { addPrompt, applyAcpEvent, startAgentChat, type AgentChat } from '../utils/acpConversation';
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
            chat, busy: false, error: null, send: noop, cancel: noop, answer: noop, authenticate: noop, end: noop, onOpenFile: noop,
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
