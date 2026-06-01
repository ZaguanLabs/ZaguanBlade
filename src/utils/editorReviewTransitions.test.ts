import { strict as assert } from 'node:assert';
import test from 'node:test';
import { getEditorReviewTransition } from './editorReviewTransitions';

test('accepted clean active review marks current editor content clean', () => {
  assert.deepEqual(getEditorReviewTransition({
    filePath: 'src/file.ts',
    reason: 'accepted',
    locallyDirty: false,
    currentContent: 'accepted model content',
  }), {
    action: 'mark-clean',
    contentState: {
      filePath: 'src/file.ts',
      savedContent: 'accepted model content',
      draftContent: undefined,
      isDirty: false,
    },
  });
});

test('accepted review does not overwrite locally dirty active editor state', () => {
  assert.deepEqual(getEditorReviewTransition({
    filePath: 'src/file.ts',
    reason: 'accepted',
    locallyDirty: true,
    currentContent: 'local dirty content',
  }), {
    action: 'ignore',
  });
});

test('rejected active review requests authoritative reload', () => {
  assert.deepEqual(getEditorReviewTransition({
    filePath: 'src/file.ts',
    reason: 'rejected',
    locallyDirty: true,
    currentContent: 'rejected content',
  }), {
    action: 'request-authoritative-reload',
  });
});

test('non-review updates are ignored by review transition helper', () => {
  assert.deepEqual(getEditorReviewTransition({
    filePath: 'src/file.ts',
    reason: 'applied',
    locallyDirty: false,
    currentContent: 'content',
  }), {
    action: 'ignore',
  });
});
