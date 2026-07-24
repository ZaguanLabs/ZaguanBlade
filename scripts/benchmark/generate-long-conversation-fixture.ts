#!/usr/bin/env bun
/**
 * Generate a synthetic long-conversation fixture for benchmarking.
 *
 * The output is a JSON StoredConversation shape with metadata and messages
 * covering every field the UI persists: plain text, reasoning, tool calls,
 * tool results, mentions, and image references.  No private data is used.
 *
 * Usage:
 *   bun scripts/benchmark/generate-long-conversation-fixture.ts \
 *     --out=benchmarks/corpora/long_chat.json --messages=10000
 */

import { mkdirSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname } from 'node:path';

interface Args {
    out: string;
    messages: number;
}

function parseArgs(): Args {
    const out =
        process.argv.find((a) => a.startsWith('--out='))?.slice('--out='.length) ??
        'benchmarks/corpora/long_chat.json';
    const messagesArg = process.argv.find((a) => a.startsWith('--messages='))?.slice('--messages='.length);
    const messages = Math.max(1, parseInt(messagesArg || '10000', 10));
    return { out, messages };
}

function sha256(text: string): string {
    return createHash('sha256').update(text).digest('hex');
}

const base64PngPixel =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';

const lorem =
    'Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.';

function makeMessage(index: number) {
    const roleCycle = index % 4;
    const role = ['user', 'assistant', 'tool', 'system'][roleCycle] as
        | 'user'
        | 'assistant'
        | 'tool'
        | 'system';

    const base = {
        id: `msg_${index.toString().padStart(6, '0')}`,
        role,
        content: `[${index}] ${role}: ${lorem.slice(0, 60 + (index % 120))}`,
    };

    const extras: Record<string, unknown> = {};

    if (role === 'user' && index % 7 === 0) {
        extras.mentions = [
            { kind: 'path', path: `src/components/Widget${index % 5}.tsx`, is_dir: false },
            { kind: 'path', path: `src/utils`, is_dir: true },
        ];
    }

    if (role === 'user' && index % 11 === 0) {
        extras.images = [
            {
                data: base64PngPixel,
                mime_type: 'image/png',
                name: `screenshot_${index}.png`,
                size: base64PngPixel.length,
            },
        ];
    }

    if (role === 'assistant') {
        extras.backend_content = base.content;

        if (index % 9 === 0) {
            extras.reasoning = `Reasoning for turn ${index}: consider the trade-offs before selecting the implementation.`;
            extras.content_before_tools = 'I think we should inspect the file first.';
            extras.content_after_tools = 'Based on the output, the fix is straightforward.';
        }

        if (index % 13 === 0) {
            extras.tool_calls = [
                {
                    id: `call_${index}`,
                    type: 'function',
                    function: {
                        name: 'read_file',
                        arguments: JSON.stringify({ path: `/workspace/src/main${index % 3}.rs` }),
                    },
                    status: 'complete',
                    result: `// contents of main${index % 3}.rs`,
                },
            ];
        }

        if (index % 17 === 0) {
            extras.progress = {
                message: 'Indexing workspace symbols',
                stage: 'indexing',
                percent: Math.min(100, Math.round((index / 1000) * 100)),
            };
        }
    }

    if (role === 'tool') {
        extras.tool_call_id = `call_${index - 1}`;
        extras.content = JSON.stringify({ ok: true, files: [`file_${index}.rs`] });
    }

    return { ...base, ...extras };
}

function main() {
    const { out, messages } = parseArgs();

    const conversationId = 'long-chat-fixture-' + sha256('zaguan-blade-long-chat-fixture-v1').slice(0, 16);
    // Fixed timestamp keeps the committed corpus hash reproducible.
    const now = '2026-07-24T00:00:00.000Z';

    const conversation = {
        version: 2,
        metadata: {
            id: conversationId,
            title: 'Long Conversation Fixture',
            created_at: now,
            updated_at: now,
            model_id: 'claude-sonnet',
            message_count: messages,
            session_id: 'fixture-session',
            planning_mode: false,
            runtime_mode: 'code',
            mode_source: 'fixture',
            format_version: 2,
        },
        messages: Array.from({ length: messages }, (_, i) => makeMessage(i)),
    };

    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, JSON.stringify(conversation, null, 2) + '\n');
    console.log(`Wrote ${messages} messages to ${out}`);
}

main();
