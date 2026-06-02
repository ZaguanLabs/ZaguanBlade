import assert from 'node:assert/strict';
import test from 'node:test';
import type { UncommittedChange } from '../types/uncommitted';
import { upsertUncommittedChangeState } from './uncommittedChangesState';

const change = (overrides: Partial<UncommittedChange>): UncommittedChange => ({
  id: 'change-1',
  file_path: '/workspace/src/file.ts',
  snapshot_id: 'snapshot-1',
  unified_diff: '--- a/file\n+++ b/file\n@@ -1 +1 @@\n-old\n+new\n',
  added_lines: 1,
  removed_lines: 1,
  timestamp: 1,
  ...overrides,
});

test('upsertUncommittedChangeState replaces stale non-empty diff with refreshed diff', () => {
  const previous = [change({ unified_diff: '+stale\n', timestamp: 1 })];
  const next = change({ unified_diff: '+current\n', timestamp: 2 });

  const result = upsertUncommittedChangeState(previous, '/workspace/src/file.ts', next);

  assert.equal(result.length, 1);
  assert.equal(result[0].unified_diff, '+current\n');
});

test('upsertUncommittedChangeState removes stale diff when backend reports no current change', () => {
  const previous = [change({ unified_diff: '+stale\n', timestamp: 1 })];

  const result = upsertUncommittedChangeState(previous, '/workspace/src/file.ts', null);

  assert.deepEqual(result, []);
});

test('upsertUncommittedChangeState removes stale diff when refreshed diff is empty', () => {
  const previous = [change({ unified_diff: '+stale\n', timestamp: 1 })];
  const next = change({ unified_diff: '', added_lines: 0, removed_lines: 0, timestamp: 2 });

  const result = upsertUncommittedChangeState(previous, '/workspace/src/file.ts', next);

  assert.deepEqual(result, []);
});
