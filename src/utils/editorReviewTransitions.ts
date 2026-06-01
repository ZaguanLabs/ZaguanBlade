import type { EditorContentSnapshot } from './editorBufferRegistry';
import type { UncommittedChangesUpdateReason } from './uncommittedChangeNotifications';

export type EditorReviewTransitionInput = {
  filePath: string;
  reason: UncommittedChangesUpdateReason;
  locallyDirty: boolean;
  currentContent: string;
};

export type EditorReviewTransitionResult =
  | {
      action: 'mark-clean';
      contentState: EditorContentSnapshot;
    }
  | {
      action: 'request-authoritative-reload';
    }
  | {
      action: 'ignore';
    };

export function getEditorReviewTransition(input: EditorReviewTransitionInput): EditorReviewTransitionResult {
  if (input.reason === 'accepted') {
    if (input.locallyDirty) {
      return { action: 'ignore' };
    }

    return {
      action: 'mark-clean',
      contentState: {
        filePath: input.filePath,
        savedContent: input.currentContent,
        draftContent: undefined,
        isDirty: false,
      },
    };
  }

  if (input.reason === 'rejected') {
    return { action: 'request-authoritative-reload' };
  }

  return { action: 'ignore' };
}
