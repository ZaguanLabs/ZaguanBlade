import type { UncommittedChange } from '../types/uncommitted';

export const normalizeUncommittedPath = (value: string): string => {
  return value.replace(/\\/g, '/').replace(/\/+$/g, '').replace(/\/+/g, '/');
};

export function upsertUncommittedChangeState(
  previous: UncommittedChange[],
  filePath: string,
  nextChange: UncommittedChange | null,
): UncommittedChange[] {
  const targetPath = normalizeUncommittedPath(filePath);
  const filtered = previous.filter(change => normalizeUncommittedPath(change.file_path) !== targetPath);

  if (!nextChange || nextChange.unified_diff.trim().length === 0) {
    return filtered;
  }

  return [...filtered, nextChange].sort((a, b) => a.timestamp - b.timestamp);
}
