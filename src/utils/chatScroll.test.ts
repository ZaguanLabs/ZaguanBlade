import assert from 'node:assert/strict';
import test from 'node:test';
import { isNearChatBottom, shouldDetachChatAutoScrollOnWheel } from './chatScroll';

test('shouldDetachChatAutoScrollOnWheel detaches on upward wheel when scrollable above', () => {
    assert.equal(shouldDetachChatAutoScrollOnWheel(-1, 20), true);
});

test('shouldDetachChatAutoScrollOnWheel does not detach on downward wheel', () => {
    assert.equal(shouldDetachChatAutoScrollOnWheel(1, 20), false);
});

test('shouldDetachChatAutoScrollOnWheel does not detach when already at top', () => {
    assert.equal(shouldDetachChatAutoScrollOnWheel(-1, 0), false);
});

test('isNearChatBottom uses threshold distance', () => {
    assert.equal(isNearChatBottom(1000, 760, 200, 80), true);
    assert.equal(isNearChatBottom(1000, 700, 200, 80), false);
});
