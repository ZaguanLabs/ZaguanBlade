import assert from 'node:assert/strict';
import test from 'node:test';
import { normalizeSplitBlocks } from '../hooks/useChatV2';
import type { ChatMessage, CommandExecution, ToolCall } from '../types/chat';
import type { StructuredAction } from '../types/events';
import { computeStableChatRows, computeStableChatTimelineRows, deriveChatActiveWorkState, deriveChatProjection, deriveChatRows, deriveChatTimelineRows, deriveChatTimelineRowsFromProjection, deriveChatWorkEntries, deriveMessageRenderSegments, stabilizeChatProjection, type ChatActivity, type StableChatRowsState, type StableChatTimelineRowsState } from './chatTimeline';
import { ensureMessagesHaveBlocks, insertAssistantMessageAfterLastUser, insertToolCallBlockPreservingOrder, moveExistingContentAfterTools, upsertSplitTextBlocks } from './messageBlocks';

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

test('computeStableChatRows reuses unchanged row objects', () => {
    const messages: ChatMessage[] = [
        { id: 'user-1', role: 'User', content: 'hi' },
        makeAssistantMessage({ id: 'assistant-1', content: 'hello' }),
    ];
    const initialRows = deriveChatRows(messages, false, null);
    const initialState: StableChatRowsState = { byKey: new Map(), rows: [] };
    const firstState = computeStableChatRows(initialRows, initialState);
    const nextRows = deriveChatRows(messages, false, null);
    const secondState = computeStableChatRows(nextRows, firstState);

    assert.equal(secondState, firstState);
    assert.equal(secondState.rows[0], firstState.rows[0]);
    assert.equal(secondState.rows[1], firstState.rows[1]);
});

test('computeStableChatRows replaces only rows whose derived fields changed', () => {
    const messages: ChatMessage[] = [
        { id: 'user-1', role: 'User', content: 'hi' },
        makeAssistantMessage({ id: 'assistant-1', content: 'hello' }),
    ];
    const initialRows = deriveChatRows(messages, false, null);
    const firstState = computeStableChatRows(initialRows, { byKey: new Map(), rows: [] });
    const activeRows = deriveChatRows(messages, true, null);
    const secondState = computeStableChatRows(activeRows, firstState);

    assert.notEqual(secondState, firstState);
    assert.equal(secondState.rows[0], firstState.rows[0]);
    assert.notEqual(secondState.rows[1], firstState.rows[1]);
    assert.equal(secondState.rows[1]?.isActive, true);
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

test('deriveMessageRenderSegments keeps pending run_command tool calls in the timeline', () => {
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

    assert.equal(segments.length, 1);
    assert.equal(segments[0]?.kind, 'activity_group');
});

test('deriveChatWorkEntries creates compact entries from ordered activity blocks', () => {
    const toolCall = makeToolCall({
        id: 'tool-1',
        function: { name: 'run_command', arguments: '{"command":"bun run build"}' },
        status: 'executing',
    });
    const commandExecution = makeCommandExecution({
        id: 'tool-1',
        command: 'bun run build',
        exitCode: 0,
    });
    const message = makeAssistantMessage({
        id: 'assistant-work',
        content: 'Running the build',
        tool_calls: [toolCall],
        commandExecutions: [commandExecution],
        blocks: [
            { type: 'tool_call', id: 'tool-1' },
            { type: 'command_execution', id: 'tool-1' },
        ],
    });

    const entries = deriveChatWorkEntries([message]);

    assert.deepEqual(
        entries.map((entry) => `${entry.source}:${entry.label}:${entry.command}`),
        [
            'tool_call:Run command:bun run build',
            'command_execution:Ran command:bun run build',
        ],
    );
    assert.equal(entries[0]?.status, 'executing');
    assert.equal(entries[0]?.messageId, 'assistant-work');
});

test('deriveChatWorkEntries falls back to message tool arrays when blocks are absent', () => {
    const message = makeAssistantMessage({
        id: 'assistant-fallback',
        content: 'Read the file',
        tool_calls: [
            makeToolCall({
                id: 'tool-read',
                function: { name: 'read_file', arguments: '{"path":"src/main.ts"}' },
            }),
        ],
    });

    const entries = deriveChatWorkEntries([message]);

    assert.equal(entries.length, 1);
    assert.equal(entries[0]?.label, 'Read File');
    assert.equal(entries[0]?.detail, 'src/main.ts');
    assert.equal(entries[0]?.toolCallId, 'tool-read');
});

test('deriveChatWorkEntries marks failed command executions as error tone', () => {
    const message = makeAssistantMessage({
        id: 'assistant-error',
        content: 'Command failed',
        commandExecutions: [
            makeCommandExecution({
                id: 'cmd-1',
                command: 'bun run lint',
                exitCode: 1,
            }),
        ],
    });

    const entries = deriveChatWorkEntries([message]);

    assert.equal(entries.length, 1);
    assert.equal(entries[0]?.tone, 'error');
    assert.equal(entries[0]?.commandExecutionId, 'cmd-1');
});

test('deriveChatTimelineRows inserts work log rows after assistant messages with work entries', () => {
    const message = makeAssistantMessage({
        id: 'assistant-timeline',
        content: 'Reading',
        tool_calls: [
            makeToolCall({
                id: 'tool-read',
                function: { name: 'read_file', arguments: '{"path":"src/main.ts"}' },
            }),
        ],
    });

    const rows = deriveChatTimelineRows([
        { id: 'user-1', role: 'User', content: 'inspect' },
        message,
    ], false, null);

    assert.deepEqual(rows.map((row) => row.kind), ['message', 'message', 'work_log']);
    assert.equal(rows[2]?.key, 'assistant-timeline:work-log');
    assert.equal(rows[2]?.kind === 'work_log' ? rows[2].entries[0]?.toolCallId : null, 'tool-read');
});

test('deriveChatProjection indexes messages and work entries separately', () => {
    const message = makeAssistantMessage({
        id: 'assistant-projection',
        content: 'Reading',
        tool_calls: [
            makeToolCall({
                id: 'tool-read',
                function: { name: 'read_file', arguments: '{"path":"src/main.ts"}' },
            }),
        ],
    });

    const projection = deriveChatProjection([
        { id: 'user-1', role: 'User', content: 'inspect' },
        message,
    ]);

    assert.equal(projection.messages.length, 2);
    assert.equal(projection.messageById.get('assistant-projection'), message);
    assert.equal(projection.workEntries.length, 1);
    assert.equal(projection.workEntriesByMessageId.get('assistant-projection')?.[0]?.toolCallId, 'tool-read');
});

test('deriveChatProjection merges explicit activity work entries', () => {
    const message = makeAssistantMessage({
        id: 'assistant-activity',
        content: 'Working',
    });
    const activity: ChatActivity = {
        id: 'activity-1',
        kind: 'tool',
        toolName: 'grep_search',
        action: 'searching',
        messageId: 'assistant-activity',
        detail: 'src/**/*.ts',
        status: 'executing',
    };

    const projection = deriveChatProjection([message], [activity]);

    assert.equal(projection.activities.length, 1);
    assert.equal(projection.activityById.get('activity-1'), activity);
    assert.equal(projection.workEntries.length, 1);
    assert.equal(projection.workEntries[0]?.source, 'activity');
    assert.equal(projection.workEntries[0]?.label, 'Grep Search');
    assert.equal(projection.workEntriesByMessageId.get('assistant-activity')?.[0]?.detail, 'src/**/*.ts');
});

test('deriveChatProjection avoids duplicating activity entries for known tool calls', () => {
    const message = makeAssistantMessage({
        id: 'assistant-activity-dedupe',
        content: 'Reading',
        tool_calls: [
            makeToolCall({
                id: 'tool-read',
                function: { name: 'read_file', arguments: '{"path":"src/main.ts"}' },
            }),
        ],
    });
    const activity: ChatActivity = {
        id: 'activity-tool-read',
        kind: 'tool',
        toolName: 'read_file',
        action: 'reading',
        toolCallId: 'tool-read',
        filePath: 'src/main.ts',
        status: 'executing',
    };

    const projection = deriveChatProjection([message], [activity]);

    assert.equal(projection.activityById.get('activity-tool-read'), activity);
    assert.equal(projection.workEntries.length, 1);
    assert.equal(projection.workEntries[0]?.source, 'tool_call');
    assert.equal(projection.workEntries[0]?.toolCallId, 'tool-read');
});

test('deriveChatTimelineRowsFromProjection uses projection work entry indexes', () => {
    const message = makeAssistantMessage({
        id: 'assistant-projected-timeline',
        content: 'Reading',
        tool_calls: [
            makeToolCall({
                id: 'tool-read',
                function: { name: 'read_file', arguments: '{"path":"src/main.ts"}' },
            }),
        ],
    });
    const projection = deriveChatProjection([message]);

    const rows = deriveChatTimelineRowsFromProjection(projection, false, null);

    assert.deepEqual(rows.map((row) => row.kind), ['message', 'work_log']);
    assert.equal(rows[1]?.kind === 'work_log' ? rows[1].entries[0] : null, projection.workEntries[0]);
});

test('stabilizeChatProjection returns previous projection when inputs and work entries are unchanged', () => {
    const message = makeAssistantMessage({
        id: 'assistant-stable-projection',
        content: 'Reading',
        tool_calls: [
            makeToolCall({
                id: 'tool-read',
                function: { name: 'read_file', arguments: '{"path":"src/main.ts"}' },
            }),
        ],
    });
    const messages = [message];
    const activities: ChatActivity[] = [];
    const firstProjection = deriveChatProjection(messages, activities);
    const secondProjection = deriveChatProjection(messages, activities);

    const stableProjection = stabilizeChatProjection(secondProjection, firstProjection);

    assert.equal(stableProjection, firstProjection);
});

test('stabilizeChatProjection reuses unchanged work entry substructures when messages change', () => {
    const firstMessage = makeAssistantMessage({
        id: 'assistant-stable-substructure',
        content: 'Reading',
        tool_calls: [
            makeToolCall({
                id: 'tool-read',
                function: { name: 'read_file', arguments: '{"path":"src/main.ts"}' },
            }),
        ],
    });
    const secondMessage = {
        ...firstMessage,
        content: 'Reading more',
    };
    const firstProjection = deriveChatProjection([firstMessage], []);
    const secondProjection = deriveChatProjection([secondMessage], []);

    const stableProjection = stabilizeChatProjection(secondProjection, firstProjection);

    assert.notEqual(stableProjection, firstProjection);
    assert.equal(stableProjection.workEntries[0], firstProjection.workEntries[0]);
    assert.equal(
        stableProjection.workEntriesByMessageId.get('assistant-stable-substructure'),
        firstProjection.workEntriesByMessageId.get('assistant-stable-substructure'),
    );
});

test('deriveChatActiveWorkState indexes pending and executing work by message', () => {
    const message = makeAssistantMessage({
        id: 'assistant-active-work',
        content: 'Working',
        tool_calls: [
            makeToolCall({
                id: 'tool-active',
                function: { name: 'read_file', arguments: '{"path":"src/main.ts"}' },
                status: 'executing',
            }),
            makeToolCall({
                id: 'tool-complete',
                function: { name: 'grep_search', arguments: '{"query":"foo"}' },
                status: 'complete',
            }),
        ],
    });
    const projection = deriveChatProjection([message]);

    const activeWorkState = deriveChatActiveWorkState(projection);

    assert.equal(activeWorkState.activeEntries.length, 1);
    assert.equal(activeWorkState.activeEntries[0]?.toolCallId, 'tool-active');
    assert.equal(activeWorkState.activeMessageIds.has('assistant-active-work'), true);
    assert.equal(activeWorkState.activeEntriesByMessageId.get('assistant-active-work')?.[0]?.toolCallId, 'tool-active');
});

test('computeStableChatTimelineRows reuses unchanged work log rows', () => {
    const message = makeAssistantMessage({
        id: 'assistant-stable-work',
        content: 'Reading',
        tool_calls: [
            makeToolCall({
                id: 'tool-read',
                function: { name: 'read_file', arguments: '{"path":"src/main.ts"}' },
            }),
        ],
    });
    const firstRows = deriveChatTimelineRows([message], false, null);
    const firstState = computeStableChatTimelineRows(firstRows, { byKey: new Map(), rows: [] } satisfies StableChatTimelineRowsState);
    const secondRows = deriveChatTimelineRows([message], false, null);
    const secondState = computeStableChatTimelineRows(secondRows, firstState);

    assert.equal(secondState, firstState);
    assert.equal(secondState.rows[0], firstState.rows[0]);
    assert.equal(secondState.rows[1], firstState.rows[1]);
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
        ['text:text-1', 'tool_call:tool-1', 'text:text-2', 'tool_call:tool-2']
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

test('upsertSplitTextBlocks reorders late tool insertion so trailing text stays after the activity group', () => {
    const blocks = [
        { type: 'text' as const, content: 'Before', id: 'text-1' },
        { type: 'text' as const, content: 'After', id: 'text-2' },
        { type: 'tool_call' as const, id: 'tool-1' },
    ];

    const nextBlocks = upsertSplitTextBlocks(blocks, 'Before', 'After');

    assert.deepEqual(
        nextBlocks.map((block) => block.type === 'text' ? `${block.type}:${block.content}` : `${block.type}:${block.id}`),
        ['text:Before', 'tool_call:tool-1', 'text:After']
    );
});

test('moveExistingContentAfterTools moves streamed assistant text below a late tool call', () => {
    const blocks = [
        { type: 'text' as const, content: 'Final answer', id: 'text-1' },
        { type: 'tool_call' as const, id: 'tool-1' },
    ];

    const reordered = moveExistingContentAfterTools(blocks, 'Final answer');

    assert.equal(reordered.contentBeforeTools, '');
    assert.equal(reordered.contentAfterTools, 'Final answer');
    assert.deepEqual(
        reordered.blocks.map((block) => block.type === 'text' ? `${block.type}:${block.content}` : `${block.type}:${block.id}`),
        ['tool_call:tool-1', 'text:Final answer']
    );
});

test('insertAssistantMessageAfterLastUser appends unseen continuation assistants at the tail once the conversation already progressed', () => {
    const messages: ChatMessage[] = [
        { id: 'user-1', role: 'User', content: 'run the build' },
        makeAssistantMessage({ id: 'assistant-1', content: 'Running checks' }),
        { id: 'tool-1', role: 'Tool', content: 'Build passed', tool_call_id: 'call-1' },
    ];

    const nextMessages = insertAssistantMessageAfterLastUser(messages, makeAssistantMessage({
        id: 'assistant-2',
        content: '',
        blocks: [{ type: 'tool_call', id: 'call-2' }],
    }));

    assert.deepEqual(
        nextMessages.map((message) => `${message.role}:${message.id}`),
        ['User:user-1', 'Assistant:assistant-1', 'Tool:tool-1', 'Assistant:assistant-2']
    );
});

test('ensureMessagesHaveBlocks preserves explicit split ordering for reloaded assistant history', () => {
    const restored = ensureMessagesHaveBlocks([
        makeAssistantMessage({
            id: 'assistant-history',
            content: 'Final answer',
            content_before_tools: '',
            content_after_tools: 'Final answer',
            tool_calls: [makeToolCall({ id: 'tool-1', status: 'complete' })],
        }),
    ]);

    assert.deepEqual(
        restored[0]?.blocks?.map((block) => block.type === 'text' ? `${block.type}:${block.content}` : `${block.type}:${block.id}`),
        ['tool_call:tool-1', 'text:Final answer']
    );
});

test('normalizeSplitBlocks keeps GPT-style final text after reasoning and tool blocks', () => {
    const message = makeAssistantMessage({
        id: 'assistant-gpt',
        content: 'Final answer',
        content_before_tools: '',
        tool_calls: [makeToolCall({ id: 'tool-1', status: 'executing' })],
        reasoning: 'Thinking through the edit',
    });
    const blocks = [
        { type: 'reasoning' as const, content: 'Thinking through the edit', id: 'reasoning-1' },
        { type: 'tool_call' as const, id: 'tool-1' },
    ];

    const normalized = normalizeSplitBlocks(message, blocks, 'Final answer');

    assert.equal(normalized.contentBeforeTools, '');
    assert.equal(normalized.contentAfterTools, 'Final answer');
    assert.deepEqual(
        normalized.blocks.map((block) => block.type === 'text' || block.type === 'reasoning'
            ? `${block.type}:${block.content}`
            : `${block.type}:${block.id}`),
        ['reasoning:Thinking through the edit', 'tool_call:tool-1', 'text:Final answer']
    );
});

test('normalizeSplitBlocks keeps trailing text after tools when live blocks temporarily omit earlier reasoning', () => {
    const message = makeAssistantMessage({
        id: 'assistant-gpt-live',
        content: 'Updated the changelog entry',
        content_before_tools: '',
        tool_calls: [makeToolCall({ id: 'tool-1', status: 'executing' })],
        reasoning: 'Checking the project and then running the build',
        blocks: [
            { type: 'reasoning', content: 'Checking the project and then running the build', id: 'reasoning-1' },
            { type: 'tool_call', id: 'tool-1' },
        ],
    });
    const liveBlocks = [
        { type: 'tool_call' as const, id: 'tool-1' },
        { type: 'command_execution' as const, id: 'tool-1' },
    ];

    const normalized = normalizeSplitBlocks(message, liveBlocks, 'Updated the changelog entry');

    assert.equal(normalized.contentBeforeTools, '');
    assert.equal(normalized.contentAfterTools, 'Updated the changelog entry');
    assert.deepEqual(
        normalized.blocks.map((block) => block.type === 'text'
            ? `${block.type}:${block.content}`
            : `${block.type}:${block.id}`),
        ['tool_call:tool-1', 'command_execution:tool-1', 'text:Updated the changelog entry']
    );
});
