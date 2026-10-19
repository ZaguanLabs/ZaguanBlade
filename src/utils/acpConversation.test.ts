import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { AcpAgentStatus, AcpToolCall } from '../types/acp';
import { AcpStartRequest, addPrompt, applyAcpEvent, runningTurn, startAgentChat, storedTurns } from './acpConversation';

const status: AcpAgentStatus = {
    conversation_id: 'c1', integration_id: 'agent', name: 'Agent', ready: true,
    snapshot: {
        protocol_version: 1, agent: null, load_session: false, resume_session: false, close_session: false,
        list_sessions: false, logout: false, prompt_image: false, prompt_audio: false, prompt_embedded_context: false,
        mcp_http: false, mcp_sse: false, auth_methods: [],
    },
};
const call = (id: string, state: AcpToolCall['status']): AcpToolCall =>
    ({ id, title: `Edit ${id}`, kind: 'edit', status: state, locations: [], content: [], truncated: false });

test('a turn collects streamed text, thoughts, merged tool reports and its stop reason', () => {
    let chat = startAgentChat(status);
    // Events can arrive before the prompt call returns its turn number.
    chat = applyAcpEvent(chat, { type: 'message_chunk', turn: 1, content: { type: 'text', text: 'Hel' } });
    chat = addPrompt(chat, 1, 'Fix it');
    chat = applyAcpEvent(chat, { type: 'message_chunk', turn: 1, content: { type: 'text', text: 'lo' } });
    chat = applyAcpEvent(chat, { type: 'message_chunk', turn: 1, content: { type: 'image', mime_type: 'image/png' } });
    chat = applyAcpEvent(chat, { type: 'thought_chunk', turn: 1, content: { type: 'text', text: 'hmm' } });
    chat = applyAcpEvent(chat, { type: 'tool_call', turn: 1, call: call('a', 'pending') });
    chat = applyAcpEvent(chat, { type: 'tool_call', turn: 1, call: call('b', 'pending') });
    chat = applyAcpEvent(chat, { type: 'tool_call', turn: 1, call: call('a', 'completed') });
    chat = applyAcpEvent(chat, { type: 'plan', turn: 1, entries: [{ content: 'Step', priority: 'high', status: 'pending' }] });
    assert.equal(runningTurn(chat)?.turn, 1);
    chat = applyAcpEvent(chat, { type: 'turn_ended', turn: 1, stop_reason: 'end_turn' });
    const [turn] = chat.turns;
    assert.equal(chat.turns.length, 1);
    assert.equal(turn.prompt, 'Fix it');
    assert.equal(turn.text, 'Hello');
    assert.equal(turn.thoughts, 'hmm');
    assert.deepEqual(turn.tools.map(tool => [tool.id, tool.status]), [['a', 'completed'], ['b', 'pending']]);
    assert.equal(turn.plan.length, 1);
    assert.equal(turn.state, 'ended');
    assert.equal(turn.stopReason, 'end_turn');
    assert.equal(runningTurn(chat), undefined);
});

test('events outside a turn attach to the latest one and are dropped before any turn', () => {
    let chat = startAgentChat(status);
    chat = applyAcpEvent(chat, { type: 'message_chunk', turn: null, content: { type: 'text', text: 'early' } });
    assert.equal(chat.turns.length, 0);
    chat = addPrompt(chat, 1, 'Go');
    chat = applyAcpEvent(chat, { type: 'turn_ended', turn: 1, stop_reason: 'end_turn' });
    chat = applyAcpEvent(chat, { type: 'tool_call', turn: null, call: call('late', 'completed') });
    assert.deepEqual(chat.turns[0].tools.map(tool => tool.id), ['late']);
});

test('permission requests and requests for input queue until resolved and an exit fails the running turn', () => {
    let chat = addPrompt(startAgentChat(status), 1, 'Work');
    const request = { request_id: 'r1', tool_call: call('a', 'pending'), options: [{ id: 'allow', name: 'Allow', kind: 'allow_once' as const }] };
    chat = applyAcpEvent(chat, { type: 'permission', turn: 1, request });
    chat = applyAcpEvent(chat, { type: 'permission', turn: 1, request });
    assert.equal(chat.permissions.length, 1, 'a repeated event does not duplicate the request');
    chat = applyAcpEvent(chat, { type: 'permission_resolved', request_id: 'r1', option_id: 'allow' });
    assert.equal(chat.permissions.length, 0);
    const input = { request_id: 'e1', message: 'Name?', tool_call_id: null, mode: 'url' as const, url: 'https://example.com/x', host: 'example.com' };
    chat = applyAcpEvent(chat, { type: 'elicitation', turn: 1, request: input });
    chat = applyAcpEvent(chat, { type: 'elicitation', turn: 1, request: input });
    chat = applyAcpEvent(chat, { type: 'elicitation', turn: 1, request: { ...input, request_id: 'e2' } });
    assert.deepEqual(chat.elicitations.map(item => item.request_id), ['e1', 'e2']);
    chat = applyAcpEvent(chat, { type: 'elicitation_resolved', request_id: 'e1', answer: 'accept' });
    assert.deepEqual(chat.elicitations.map(item => item.request_id), ['e2']);
    chat = applyAcpEvent(chat, { type: 'permission', turn: 1, request: { ...request, request_id: 'r2' } });
    chat = applyAcpEvent(chat, { type: 'exited', error: null });
    assert.deepEqual(chat.exited, { error: null });
    assert.equal(chat.permissions.length, 0);
    assert.equal(chat.elicitations.length, 0);
    assert.equal(chat.turns[0].state, 'failed');
    assert.equal(chat.turns[0].error, 'agent_exited');
});

test('a disposed start request cancels its approval and never starts the agent', async () => {
    const calls: Array<[string, Record<string, unknown>]> = [];
    const invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
        calls.push([command, args]);
        return (command === 'prepare_acp_conversation' ? { ticket_id: 't1', kind: 'process', executable: '/bin/agent', cwd: '/w', args: [] } : null) as T;
    };
    const request = new AcpStartRequest(invoke);
    assert.equal((await request.prepare('agent', 'rev', '/w'))?.ticket_id, 't1');
    request.dispose();
    assert.equal(await request.start(), null);
    assert.deepEqual(calls.map(([command]) => command), ['prepare_acp_conversation', 'cancel_integration_test']);
    assert.deepEqual(calls[1][1], { ticketId: 't1' });
});

test('a reconnected chat shows the stored transcript before its own turns and never changes it', () => {
    const history = storedTurns([
        { role: 'User', content: 'First' },
        { role: 'Assistant', content: 'One', reasoning: 'Thinking' },
        { role: 'Assistant', content: 'More' },
        { role: 'System', content: 'ignored' },
        { role: 'User', content: 'Second' },
    ]);
    assert.deepEqual(history.map(turn => [turn.turn, turn.prompt, turn.text, turn.thoughts, turn.state, turn.stored]), [
        [-3, 'First', 'One', 'Thinking', 'ended', true],
        [-2, '', 'More', '', 'ended', true],
        [-1, 'Second', '', '', 'ended', true],
    ]);
    let chat = startAgentChat(status, history);
    // An update outside any turn has no live turn to go to.
    chat = applyAcpEvent(chat, { type: 'message_chunk', turn: null, content: { type: 'text', text: 'stray' } });
    assert.equal(chat.turns[2].text, '');
    chat = addPrompt(chat, 1, 'Third');
    chat = applyAcpEvent(chat, { type: 'message_chunk', turn: null, content: { type: 'text', text: 'Three' } });
    assert.deepEqual(chat.turns.map(turn => turn.text), ['One', 'More', '', 'Three']);
});

test('reconnecting starts the agent for the stored conversation', async () => {
    const calls: Array<[string, Record<string, unknown>]> = [];
    const invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
        calls.push([command, args]);
        return (command === 'prepare_acp_conversation' ? { ticket_id: 't1', kind: 'process', executable: '/bin/agent', cwd: '/w', args: [] } : status) as T;
    };
    const request = new AcpStartRequest(invoke);
    await request.prepare('agent', 'rev', '/w');
    assert.equal(await request.start('c1'), status);
    assert.deepEqual(calls[1], ['start_acp_conversation', { ticketId: 't1', conversationId: 'c1' }]);
});

test('stored turns keep the reports the agent made in them', () => {
    const [turn, failed] = storedTurns([
        { role: 'User', content: 'Fix it', images: [{ data: 'AAAA', mime_type: 'image/png' }] },
        { role: 'Assistant', content: 'Fixed.', agent_activity: {
            agent_name: 'Agent', tool_calls: [call('main.rs', 'completed')], omitted_tool_calls: 3,
            plan: [{ content: 'Fix', priority: 'high', status: 'completed' }], stop_reason: 'max_tokens', text_truncated: true,
        } },
        { role: 'User', content: 'Again' },
        { role: 'Assistant', content: '', agent_activity: { agent_name: 'Agent', error: 'agent_exited' } },
    ]);
    assert.deepEqual(turn.images, [{ data: 'AAAA', mime_type: 'image/png' }]);
    assert.deepEqual(turn.tools.map(tool => tool.id), ['main.rs']);
    assert.equal(turn.omittedTools, 3);
    assert.equal(turn.plan.length, 1);
    assert.deepEqual([turn.state, turn.stopReason, turn.truncated], ['ended', 'max_tokens', true]);
    assert.deepEqual([failed.state, failed.error], ['failed', 'agent_exited']);
});
