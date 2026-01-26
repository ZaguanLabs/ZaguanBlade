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
    <div className="flex items-center justify-end gap-2 px-3 py-1.5 text-xs">
      <div className="flex items-center gap-1.5 text-[var(--fg-secondary)]">
        <FileCode className="w-3 h-3" />
        <span>{fileCount} file{fileCount !== 1 ? 's' : ''}</span>
        <span className="text-green-500">+{totalAdded}</span>
        <span className="text-red-500">-{totalRemoved}</span>
      </div>

      <div className="w-px h-4 bg-[var(--border-primary)]" />

      <div className="flex items-center gap-1.5">
        <button
          onClick={onAcceptAll}
          disabled={disabled}
          className="flex items-center gap-1 px-2 py-0.5 font-medium text-green-400 bg-green-500/10 hover:bg-green-500/20 rounded transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          title="Accept all AI changes"
        >
          <Check className="w-3 h-3" />
          Accept All
        </button>
        <button
          onClick={onRejectAll}
          disabled={disabled}
          className="flex items-center gap-1 px-2 py-0.5 font-medium text-red-400 bg-red-500/10 hover:bg-red-500/20 rounded transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          title="Reject all AI changes (revert to original)"
        >
          <X className="w-3 h-3" />
          Reject All
        </button>
      </div>
    </div>
  );
};

export default GlobalChangeActions;
