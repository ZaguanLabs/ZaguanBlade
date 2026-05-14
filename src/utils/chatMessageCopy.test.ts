import assert from 'node:assert/strict';
import test from 'node:test';
import { getCopyableMessageContent } from './chatMessageCopy';

test('getCopyableMessageContent strips accidental plain role prefix', () => {
    assert.equal(
        getCopyableMessageContent({ role: 'User', content: 'User\n\nActual message' }),
        'Actual message',
    );
});

test('getCopyableMessageContent strips accidental markdown role prefix', () => {
    assert.equal(
        getCopyableMessageContent({ role: 'Assistant', content: '**Assistant:**\n\nActual message' }),
        'Actual message',
    );
});

test('getCopyableMessageContent preserves normal body content', () => {
    assert.equal(
        getCopyableMessageContent({ role: 'User', content: 'Actual message\n\nWith spacing' }),
        'Actual message\n\nWith spacing',
    );
});
