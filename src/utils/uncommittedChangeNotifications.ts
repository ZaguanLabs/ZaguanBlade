export type UncommittedChangesUpdateReason = 'applied' | 'accepted' | 'rejected' | 'refresh';

export interface UncommittedChangesUpdatedDetail {
  sourceId?: string;
  filePaths?: string[];
  reason?: UncommittedChangesUpdateReason;
}

export type UncommittedFileChangedCallback = (filePath: string, reason: UncommittedChangesUpdateReason) => void;

export function notifyUncommittedFilesChanged(
  filePaths: string[],
  reason: UncommittedChangesUpdateReason,
  onFileChanged?: UncommittedFileChangedCallback,
) {
  if (!onFileChanged) {
    return;
  }

  filePaths.forEach(filePath => onFileChanged(filePath, reason));
}

export function createUncommittedChangesUpdatedEvent(
  sourceId: string,
  filePaths?: string[],
  reason: UncommittedChangesUpdateReason = 'refresh',
) {
  return new CustomEvent<UncommittedChangesUpdatedDetail>('uncommitted-changes-updated', {
    detail: { sourceId, filePaths, reason },
  });
}
