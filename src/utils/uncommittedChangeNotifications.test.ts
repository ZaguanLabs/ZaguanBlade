import { strict as assert } from 'node:assert';
import test from 'node:test';
import {
  createUncommittedChangesUpdatedEvent,
  notifyUncommittedFilesChanged,
  type UncommittedChangesUpdateReason,
} from './uncommittedChangeNotifications';

test('notifyUncommittedFilesChanged immediately reports accepted and rejected reasons for every file', () => {
  const calls: Array<{ filePath: string; reason: UncommittedChangesUpdateReason }> = [];

  notifyUncommittedFilesChanged(['src/a.ts', 'src/b.ts'], 'accepted', (filePath, reason) => {
    calls.push({ filePath, reason });
  });
  notifyUncommittedFilesChanged(['src/c.ts'], 'rejected', (filePath, reason) => {
    calls.push({ filePath, reason });
  });

  assert.deepEqual(calls, [
    { filePath: 'src/a.ts', reason: 'accepted' },
    { filePath: 'src/b.ts', reason: 'accepted' },
    { filePath: 'src/c.ts', reason: 'rejected' },
  ]);
});

test('notifyUncommittedFilesChanged is a no-op without a callback', () => {
  assert.doesNotThrow(() => {
    notifyUncommittedFilesChanged(['src/a.ts'], 'rejected');
  });
});

test('createUncommittedChangesUpdatedEvent preserves source, paths, and semantic reason', () => {
  const event = createUncommittedChangesUpdatedEvent('source-1', ['src/file.ts'], 'rejected');

  assert.equal(event.type, 'uncommitted-changes-updated');
  assert.deepEqual(event.detail, {
    sourceId: 'source-1',
    filePaths: ['src/file.ts'],
    reason: 'rejected',
  });
});

test('createUncommittedChangesUpdatedEvent preserves accepted multi-file updates', () => {
  const event = createUncommittedChangesUpdatedEvent('git-push', ['src/a.ts', 'src/b.ts'], 'accepted');

  assert.deepEqual(event.detail, {
    sourceId: 'git-push',
    filePaths: ['src/a.ts', 'src/b.ts'],
    reason: 'accepted',
  });
});
