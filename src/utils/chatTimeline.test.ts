import assert from 'node:assert/strict';
import test from 'node:test';
import type { ChatMessage, CommandExecution, ToolCall } from '../types/chat';
import type { StructuredAction } from '../types/events';
import { deriveChatRows, deriveMessageRenderSegments } from './chatTimeline';
import { insertToolCallBlockPreservingOrder, upsertSplitTextBlocks } from './messageBlocks';

function makeToolCall(overrides: Partial<ToolCall> & Pick<ToolCall, 'id'>): ToolCall {
    return {
        id: overrides.id,
        type: overrides.type ?? 'function',
        function: overrides.function ?? {
            name: 'grep_search',
            arguments: '{}',
        },
        status: overrides.status,
        result: overrides.result,
    };
}

function makeCommandExecution(overrides: Partial<CommandExecution> & Pick<CommandExecution, 'id' | 'command'>): CommandExecution {
    return {
        id: overrides.id,
        command: overrides.command,
        cwd: overrides.cwd,
        output: overrides.output ?? '',
        exitCode: overrides.exitCode ?? 0,
        duration: overrides.duration,
        timestamp: overrides.timestamp ?? Date.now(),
    };
}

function makeAssistantMessage(overrides: Partial<ChatMessage> & Pick<ChatMessage, 'id' | 'content'>): ChatMessage {
    return {
        id: overrides.id,
        role: 'Assistant',
        content: overrides.content,
        blocks: overrides.blocks,
        tool_calls: overrides.tool_calls,
        commandExecutions: overrides.commandExecutions,
        content_before_tools: overrides.content_before_tools,
        content_after_tools: overrides.content_after_tools,
        reasoning: overrides.reasoning,
    };
}

function makePendingAction(id: string): StructuredAction {
    return {
        id,
        command: 'bun run lint',
        description: 'bun run lint',
        is_generic_tool: false,
    };
}

test('deriveChatRows attaches pending actions to the assistant message owning the matching tool call', () => {
    const messages: ChatMessage[] = [
        { id: 'user-1', role: 'User', content: 'run lint' },
        makeAssistantMessage({
            id: 'assistant-1',
            content: 'Running checks',
            tool_calls: [
                makeToolCall({
                    id: 'call-1',
                    function: { name: 'run_command', arguments: '{"command":"bun run lint"}' },
                }),
            ],
        }),
        makeAssistantMessage({ id: 'assistant-2', content: 'Waiting for approval' }),
    ];

    const rows = deriveChatRows(messages, true, [makePendingAction('call-1')]);

    assert.equal(rows[1]?.pendingActions?.[0]?.id, 'call-1');
    assert.equal(rows[2]?.pendingActions, undefined);
    assert.equal(rows[2]?.isActive, true);
});

test('deriveChatRows falls back to the latest assistant message when no tool call match exists', () => {
    const messages: ChatMessage[] = [
        { id: 'user-1', role: 'User', content: 'hi' },
        makeAssistantMessage({ id: 'assistant-1', content: 'one' }),
        makeAssistantMessage({ id: 'assistant-2', content: 'two' }),
    ];

    const rows = deriveChatRows(messages, false, [makePendingAction('missing-call')]);

    assert.equal(rows[1]?.pendingActions, undefined);
    assert.equal(rows[2]?.pendingActions?.[0]?.id, 'missing-call');
});

test('deriveMessageRenderSegments groups tool calls and command executions together', () => {
    const toolCall = makeToolCall({ id: 'tool-1' });
    const commandExecution = makeCommandExecution({ id: 'tool-1', command: 'rg TODO src' });
    const message = makeAssistantMessage({
        id: 'assistant-1',
        content: 'Done',
        blocks: [
            { type: 'text', content: 'Before', id: 'text-1' },
            { type: 'tool_call', id: 'tool-1' },
            { type: 'command_execution', id: 'tool-1' },
            { type: 'text', content: 'After', id: 'text-2' },
        ],
        tool_calls: [toolCall],
        commandExecutions: [commandExecution],
    });

    const segments = deriveMessageRenderSegments(message, null);

    assert.equal(segments.length, 3);
    assert.equal(segments[0]?.kind, 'block');
    assert.equal(segments[1]?.kind, 'activity_group');
    assert.equal(segments[2]?.kind, 'block');
    if (segments[1]?.kind !== 'activity_group') {
        throw new Error('Expected activity group in the middle segment');
    }
    assert.deepEqual(
        segments[1].items.map((item) => item.kind),
        ['tool_call', 'command_execution']
    );
});

test('deriveMessageRenderSegments hides pending run_command tool calls behind the approval card', () => {
    const toolCall = makeToolCall({
        id: 'call-approval',
        function: { name: 'run_command', arguments: '{"command":"pwd"}' },
    });
    const message = makeAssistantMessage({
        id: 'assistant-approval',
        content: 'Need approval',
        blocks: [{ type: 'tool_call', id: 'call-approval' }],
        tool_calls: [toolCall],
    });

    const segments = deriveMessageRenderSegments(message, [makePendingAction('call-approval')]);

    assert.equal(segments.length, 0);
});

test('insertToolCallBlockPreservingOrder appends new tool calls after existing assistant text', () => {
    const blocks = [
        { type: 'text' as const, content: 'First response', id: 'text-1' },
        { type: 'tool_call' as const, id: 'tool-1' },
        { type: 'text' as const, content: 'Follow-up response', id: 'text-2' },
    ];

    const nextBlocks = insertToolCallBlockPreservingOrder(blocks, 'tool-2');

    assert.deepEqual(
        nextBlocks.map((block) => block.type === 'text' ? `${block.type}:${block.id}` : `${block.type}:${block.id}`),
        ['text:text-1', 'tool_call:tool-1', 'tool_call:tool-2', 'text:text-2']
    );
});

test('insertToolCallBlockPreservingOrder inserts a tool call before its matching command execution', () => {
    const blocks = [
        { type: 'text' as const, content: 'First response', id: 'text-1' },
        { type: 'command_execution' as const, id: 'tool-2' },
        { type: 'text' as const, content: 'After execution', id: 'text-2' },
    ];

    const nextBlocks = insertToolCallBlockPreservingOrder(blocks, 'tool-2');

    assert.deepEqual(
        nextBlocks.map((block) => block.type === 'text' ? `${block.type}:${block.id}` : `${block.type}:${block.id}`),
        ['text:text-1', 'tool_call:tool-2', 'command_execution:tool-2', 'text:text-2']
    );
});

test('upsertSplitTextBlocks keeps post-tool text after the activity group', () => {
    const blocks = [
        { type: 'text' as const, content: 'Before and after', id: 'text-1' },
        { type: 'tool_call' as const, id: 'tool-1' },
    ];

    const nextBlocks = upsertSplitTextBlocks(blocks, 'Before', ' and after');

    assert.deepEqual(
        nextBlocks.map((block) => block.type === 'text' ? `${block.type}:${block.content}` : `${block.type}:${block.id}`),
        ['text:Before', 'tool_call:tool-1', 'text: and after']
    );
});
