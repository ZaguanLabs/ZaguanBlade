import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import { EventBuffer, MessageBuffer } from './eventBuffer';

describe('EventBuffer', () => {
    test('buffers out-of-order chunks until the expected sequence arrives', () => {
        const applied: Array<{ seq: number; value: string }> = [];
        const buffer = new EventBuffer<string>((value, _isFinal, seq) => {
            applied.push({ seq: seq ?? -1, value });
        });

        buffer.add(1, 'second');
        assert.deepEqual(applied, []);

        buffer.add(0, 'first');
        assert.deepEqual(applied, [
            { seq: 0, value: 'first' },
            { seq: 1, value: 'second' },
        ]);
    });
});

describe('MessageBuffer', () => {
    test('adopts the first observed sequence for a new chat stream', () => {
        const chunks: Array<{ id: string; seq: number; chunk: string }> = [];
        const buffer = new MessageBuffer((id, seq, chunk) => {
            chunks.push({ id, seq, chunk });
        });

        buffer.addMessageDelta('assistant-1', 1, 'hello', false);
        buffer.addMessageDelta('assistant-1', 2, ' world', false);

        assert.deepEqual(chunks, [
            { id: 'assistant-1', seq: 1, chunk: 'hello' },
            { id: 'assistant-1', seq: 2, chunk: ' world' },
        ]);
    });
});
