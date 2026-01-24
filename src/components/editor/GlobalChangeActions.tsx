import React from 'react';
import { Check, X, FileCode } from 'lucide-react';
import type { UncommittedChange } from '../../types/uncommitted';

interface GlobalChangeActionsProps {
  changes: UncommittedChange[];
  onAcceptAll: () => void;
  onRejectAll: () => void;
  disabled?: boolean;
}

export const GlobalChangeActions: React.FC<GlobalChangeActionsProps> = ({
  changes,
  onAcceptAll,
  onRejectAll,
  disabled = false,
}) => {
  if (changes.length === 0) {
    return null;
  }

  const totalAdded = changes.reduce((sum, c) => sum + c.added_lines, 0);
  const totalRemoved = changes.reduce((sum, c) => sum + c.removed_lines, 0);
  const fileCount = new Set(changes.map(c => c.file_path)).size;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex items-center gap-3 px-4 py-2.5 bg-[var(--bg-secondary)] border border-[var(--border-primary)] rounded-lg shadow-lg">
      <div className="flex items-center gap-2 text-sm text-[var(--fg-secondary)]">
        <FileCode className="w-4 h-4" />
        <span>
          {fileCount} {fileCount === 1 ? 'file' : 'files'} changed
        </span>
        <span className="text-green-500">+{totalAdded}</span>
        <span className="text-red-500">-{totalRemoved}</span>
      </div>

      <div className="w-px h-5 bg-[var(--border-primary)]" />

      <div className="flex items-center gap-2">
        <button
          onClick={onAcceptAll}
          disabled={disabled}
          className="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium text-green-400 bg-green-500/10 hover:bg-green-500/20 rounded transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          title="Accept all AI changes"
        >
          <Check className="w-4 h-4" />
          Accept All
        </button>
        <button
          onClick={onRejectAll}
          disabled={disabled}
          className="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium text-red-400 bg-red-500/10 hover:bg-red-500/20 rounded transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          title="Reject all AI changes (revert to original)"
        >
          <X className="w-4 h-4" />
          Reject All
        </button>
      </div>
    </div>
  );
};

export default GlobalChangeActions;
